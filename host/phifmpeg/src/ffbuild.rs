//! `phifmpeg build`: FFmpeg for the card, from the pristine tree, with
//! configure flags as the only input.
//!
//! Two variants, each an out-of-tree build under `<build>/build/<name>`:
//!
//! - `c`: no assembly at all. Every instruction is one the card executes;
//!   the stack's `phi-isa-audit` must report zero illegal instructions or the
//!   build fails. This is the reference and the fallback.
//! - `asm`: FFmpeg's own x86 SIMD sources (SSE2 to AVX2) assembled with
//!   nasm. The card has none of those instructions, so they only run through
//!   the translator; the audit is reported, not enforced, because the hits
//!   are expected and must all sit inside FFmpeg's assembly functions.
//!
//! Inline assembly is off in both: it is embedded in C functions (CABAC
//! uses CMOV there), where no function boundary exists to translate at.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::fetch::{ffmpeg_pristine, jobs};
use crate::layout::Layout;
use crate::pins::Pins;
use crate::run;
use crate::stack::Stack;

/// Which FFmpeg build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Variant {
    /// C only, audited clean.
    C,
    /// With FFmpeg's x86 assembly, for the translator.
    Asm,
}

impl Variant {
    /// Directory and file-name stem.
    pub fn name(self) -> &'static str {
        match self {
            Variant::C => "c",
            Variant::Asm => "asm",
        }
    }
}

/// Configure arguments for a variant. The same list is written to
/// `<dir>/phifmpeg.flags`; a difference forces a fresh configure.
fn configure_args(v: Variant, lay: &Layout, dir: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec![
        format!("--prefix={}", dir.join("install").display()),
        // Cross build: the card is x86-64 Linux, but configure must not run
        // what it builds as if the host were the target.
        "--enable-cross-compile".into(),
        "--target-os=linux".into(),
        "--arch=x86_64".into(),
        // The stack's compiler driver (knc64-x87 ABI, no SSE, no CMOV) and
        // the matching LLVM binutils, all found on the imported PATH.
        "--cc=knc-cc".into(),
        "--cxx=knc-c++".into(),
        "--ar=llvm-ar".into(),
        "--ranlib=llvm-ranlib".into(),
        "--nm=llvm-nm".into(),
        "--strip=llvm-strip".into(),
        // Nothing from the host may leak in: no pkg-config, no autodetected
        // host libraries.
        "--pkg-config=false".into(),
        "--disable-autodetect".into(),
        // The card userland is static musl (no dynamic loader).
        "--enable-static".into(),
        "--disable-shared".into(),
        "--extra-ldflags=-static".into(),
        "--disable-doc".into(),
        "--enable-pthreads".into(),
        "--enable-zlib".into(),
        "--disable-inline-asm".into(),
    ];
    match v {
        Variant::C => {
            a.push("--disable-asm".into());
            a.push("--disable-x86asm".into());
        }
        Variant::Asm => {
            a.push(format!(
                "--x86asmexe={}",
                lay.tools().join("bin/nasm").display()
            ));
        }
    }
    a
}

/// Configure (when needed), build and audit one variant. Returns the
/// unstripped `ffmpeg_g`.
pub fn build(stack: &Stack, lay: &Layout, pins: &Pins, v: Variant) -> Result<PathBuf> {
    ffmpeg_pristine(lay, pins)?;
    if v == Variant::Asm && !lay.tools().join("bin/nasm").is_file() {
        bail!(
            "nasm missing in {}; run `phifmpeg fetch`",
            lay.tools().display()
        );
    }
    let env = stack.toolchain_env()?;
    let dir = lay.variant(v.name());
    std::fs::create_dir_all(&dir)?;
    let args = configure_args(v, lay, &dir);
    let flags_file = dir.join("phifmpeg.flags");
    let want = args.join("\n");
    let have = std::fs::read_to_string(&flags_file).unwrap_or_default();
    if have != want || !dir.join("ffbuild/config.mak").is_file() {
        println!("== configuring {} in {}", v.name(), dir.display());
        let log = dir.join("configure.log");
        let out = std::fs::File::create(&log)?;
        let status = Command::new(lay.ffmpeg_src().join("configure"))
            .args(&args)
            .current_dir(&dir)
            .env_clear()
            .envs(&env)
            .stdout(out.try_clone()?)
            .stderr(out)
            .status()
            .context("running FFmpeg's configure")?;
        if !status.success() {
            bail!("configure failed; see {}", log.display());
        }
        std::fs::write(&flags_file, &want)?;
    }
    println!("== building {} with {} jobs", v.name(), jobs());
    let log = dir.join("make.log");
    let out = std::fs::File::create(&log)?;
    let status = Command::new("make")
        .arg(format!("-j{}", jobs()))
        .current_dir(&dir)
        .env_clear()
        .envs(&env)
        .stdout(out.try_clone()?)
        .stderr(out)
        .status()
        .context("running make")?;
    if !status.success() {
        bail!("make failed; see {}", log.display());
    }
    let bin = dir.join("ffmpeg_g");
    audit(stack, &bin, v)?;
    Ok(bin)
}

/// Run the stack's ISA audit. Enforced for `c`, reported for `asm`.
pub fn audit(stack: &Stack, bin: &Path, v: Variant) -> Result<()> {
    let tool = stack.isa_audit()?;
    println!("== phi-isa-audit {}", bin.display());
    let mut cmd = Command::new(tool);
    cmd.arg(bin);
    match v {
        Variant::C => run(&mut cmd),
        Variant::Asm => {
            // Hits are expected: FFmpeg's SIMD functions. Report them.
            let _ = cmd.status();
            Ok(())
        }
    }
}
