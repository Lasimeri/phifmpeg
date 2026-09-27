//! Finding the stack (Intel-Phi-3120A) and borrowing its card toolchain.
//!
//! The stack owns the cross compiler (`knc-cc`, the patched LLVM, the musl
//! sysroot), the ISA audit and the `phi` command. Nothing of it is copied
//! here; it is found in the family's order and its `toolchain/env.sh` is
//! sourced once to import the environment its own build scripts use.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

/// The two names a checkout of the stack goes by: the git clone's and the
/// spaced one the project was created under.
const NAMES: [&str; 2] = ["Intel-Phi-3120A", "Intel Phi 3120A"];

/// A located stack checkout.
pub struct Stack {
    /// Repository root (may contain spaces; never hand it to a build system
    /// unquoted, use [`Stack::toolchain_env`]'s `phi_root` instead).
    pub root: PathBuf,
    /// How it was found, for `phifmpeg stack`.
    pub found_by: &'static str,
}

impl Stack {
    /// Locate the stack: `PHI_STACK_ROOT`, then the `phi` command on PATH
    /// (a symlink to `<stack>/scripts/phi.sh`), then a checkout next to this
    /// repository, then `$HOME`.
    pub fn find(repo: &Path) -> Result<Stack> {
        if let Some(p) = std::env::var_os("PHI_STACK_ROOT") {
            let p = PathBuf::from(p);
            if is_stack(&p) {
                return Ok(Stack {
                    root: p,
                    found_by: "PHI_STACK_ROOT",
                });
            }
            bail!(
                "PHI_STACK_ROOT={} is not an Intel-Phi-3120A checkout",
                p.display()
            );
        }
        if let Some(root) = from_path_command() {
            return Ok(Stack {
                root,
                found_by: "phi on PATH",
            });
        }
        if let Some(parent) = repo.parent() {
            for n in NAMES {
                let p = parent.join(n);
                if is_stack(&p) {
                    return Ok(Stack {
                        root: p,
                        found_by: "next to this checkout",
                    });
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            for n in NAMES {
                let p = PathBuf::from(&home).join(n);
                if is_stack(&p) {
                    return Ok(Stack {
                        root: p,
                        found_by: "$HOME",
                    });
                }
            }
        }
        bail!(
            "Intel-Phi-3120A not found (set PHI_STACK_ROOT, put `phi` on PATH, or clone it next to this repository)"
        )
    }

    /// The environment the stack's own card build scripts run in: `PATH`
    /// with `knc-cc` and the patched LLVM first, `PHI_LLVM`, `PHI_SYSROOT`,
    /// `CC`, `CXX`, and `phi_root` (a space-free alias of the stack).
    pub fn toolchain_env(&self) -> Result<HashMap<String, String>> {
        let out = Command::new("bash")
            .arg("-c")
            .arg(r#". "$1/toolchain/env.sh" >/dev/null 2>&1 && env -0"#)
            .arg("_")
            .arg(&self.root)
            .output()
            .context("running bash to source the stack's toolchain/env.sh")?;
        if !out.status.success() {
            bail!("sourcing {}/toolchain/env.sh failed", self.root.display());
        }
        let mut env = HashMap::new();
        for kv in out.stdout.split(|&b| b == 0) {
            let kv = String::from_utf8_lossy(kv);
            if let Some((k, v)) = kv.split_once('=') {
                env.insert(k.to_string(), v.to_string());
            }
        }
        for k in ["PATH", "PHI_LLVM", "PHI_SYSROOT", "phi_root"] {
            if !env.contains_key(k) {
                bail!("the stack's toolchain/env.sh did not export {k}");
            }
        }
        Ok(env)
    }

    /// The stack's `phi-isa-audit` (built by its `make build`).
    pub fn isa_audit(&self) -> Result<PathBuf> {
        for profile in ["debug", "release"] {
            let p = self
                .root
                .join("host/target")
                .join(profile)
                .join("phi-isa-audit");
            if p.is_file() {
                return Ok(p);
            }
        }
        bail!(
            "phi-isa-audit not built in {}/host/target (run `make build` there)",
            self.root.display()
        )
    }
}

/// True when `p` looks like a stack checkout.
fn is_stack(p: &Path) -> bool {
    p.join("toolchain/env.sh").is_file() && p.join("scripts/phi.sh").is_file()
}

/// Resolve `phi` on PATH to the stack root, if it is the stack's script.
fn from_path_command() -> Option<PathBuf> {
    let path: OsString = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("phi");
        if let Ok(real) = std::fs::canonicalize(&cand) {
            // <stack>/scripts/phi.sh
            let root = real.parent()?.parent()?.to_path_buf();
            if is_stack(&root) {
                return Some(root);
            }
        }
    }
    None
}
