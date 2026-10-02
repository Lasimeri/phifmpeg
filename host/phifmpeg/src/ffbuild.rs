//! `phifmpeg build`: x265 and FFmpeg from the pristine trees, with
//! build-system flags as the only input.
//!
//! Two variants; each builds x265 into `<build>/prefix/<name>` and FFmpeg
//! (linked with that libx265) out of tree in `<build>/build/<name>`:
//!
//! - `c` (the cards): no assembly at all, FFmpeg's inline assembly
//!   included. Every instruction is one the card executes; the stack's
//!   `phi-isa-audit` must report zero illegal instructions or the build
//!   fails. It also builds the card workspace (`card/`: the `phix` runtime,
//!   linked into both binaries by reference, and the `phifmpeg-card`
//!   runner). This is what `transcode` runs on the cards.
//! - `host`: a native build for the host with all of both projects' SIMD
//!   and FFmpeg's inline assembly; the host's share of a transcode runs it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::fetch::pristine;
use crate::layout::Layout;
use crate::pins::Pins;
use crate::stack::Stack;
use crate::util::{jobs, run};

/// Which build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Variant {
    /// The cards: C only, audited clean, with the card workspace.
    C,
    /// For the host itself, with all of both projects' SIMD: the host's
    /// share of a split transcode runs the same pinned sources as the cards.
    Host,
}

impl Variant {
    /// Directory and file-name stem.
    pub fn name(self) -> &'static str {
        match self {
            Variant::C => "c",
            Variant::Host => "host",
        }
    }
}

/// Where the card workspace's release build lands, `libphix.a` and the
/// `phifmpeg-card` runner: in the repository (git-ignored), not under the
/// build root, because cargo puts a target's output under its workspace.
/// `transcode` takes the runner from here.
pub fn card_target(repo: &Path) -> PathBuf {
    repo.join("card/target/x86_64-knc-linux-musl/release")
}

/// CMake arguments for x265.
fn x265_args(
    v: Variant,
    lay: &Layout,
    env: &HashMap<String, String>,
    phix: Option<&Path>,
) -> Vec<String> {
    let nasm = format!(
        "-DCMAKE_ASM_NASM_COMPILER={}",
        lay.tools().join("bin/nasm").display()
    );
    let prefix = format!(
        "-DCMAKE_INSTALL_PREFIX={}",
        lay.x265_prefix(v.name()).display()
    );
    let Some(phix) = phix else {
        // The host variant: a native build, the host's own compilers.
        return vec![
            "-G".into(),
            "Ninja".into(),
            "-DCMAKE_BUILD_TYPE=Release".into(),
            prefix,
            "-DENABLE_SHARED=OFF".into(),
            "-DENABLE_CLI=ON".into(),
            "-DENABLE_LIBNUMA=OFF".into(),
            "-DENABLE_ASSEMBLY=ON".into(),
            nasm,
        ];
    };
    let which = |tool: &str| -> String {
        // Absolute paths: CMake caches them, and the imported PATH is only
        // guaranteed while this process runs.
        let path = env.get("PATH").map(String::as_str).unwrap_or("");
        std::env::split_paths(path)
            .map(|d| d.join(tool))
            .find(|p| p.is_file())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| tool.to_string())
    };
    vec![
        "-G".into(),
        "Ninja".into(),
        "-DCMAKE_BUILD_TYPE=Release".into(),
        // A cross build for x86-64 Linux: CMake must not try to run
        // target programs or pick host libraries.
        "-DCMAKE_SYSTEM_NAME=Linux".into(),
        "-DCMAKE_SYSTEM_PROCESSOR=x86_64".into(),
        format!("-DCMAKE_C_COMPILER={}", which("knc-cc")),
        format!("-DCMAKE_CXX_COMPILER={}", which("knc-c++")),
        format!("-DCMAKE_AR={}", which("llvm-ar")),
        format!("-DCMAKE_RANLIB={}", which("llvm-ranlib")),
        prefix,
        // Static library for FFmpeg, and the CLI for encode-only timing.
        "-DENABLE_SHARED=OFF".into(),
        "-DENABLE_CLI=ON".into(),
        // Static, with the card runtime linked in by reference (card/phix).
        format!(
            "-DCMAKE_EXE_LINKER_FLAGS=-static -Wl,--undefined=phix_anchor -L{} -lphix",
            phix.display()
        ),
        // No libnuma in the card sysroot (one node anyway).
        "-DENABLE_LIBNUMA=OFF".into(),
        // The card has none of x265's x86 SIMD instructions.
        "-DENABLE_ASSEMBLY=OFF".into(),
    ]
}

/// FFmpeg configure arguments: the card's when `phix` (the card runtime's
/// directory) is given, the host's otherwise.
fn configure_args(lay: &Layout, dir: &Path, phix: Option<&Path>) -> Vec<String> {
    let nasm = format!("--x86asmexe={}", lay.tools().join("bin/nasm").display());
    let Some(phix) = phix else {
        // The host variant: native, SIMD and inline assembly on, x265 from
        // our prefix (pkg-config confined to it in `build`).
        return vec![
            format!("--prefix={}", dir.join("install").display()),
            "--pkg-config-flags=--static".into(),
            "--disable-autodetect".into(),
            "--enable-static".into(),
            "--disable-shared".into(),
            "--disable-doc".into(),
            "--enable-pthreads".into(),
            "--enable-zlib".into(),
            "--enable-gpl".into(),
            "--enable-libx265".into(),
            nasm,
        ];
    };
    vec![
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
        // pkg-config is confined to this variant's x265 prefix
        // (PKG_CONFIG_LIBDIR, set in `build`), so no host library leaks in.
        "--pkg-config=pkg-config".into(),
        "--pkg-config-flags=--static".into(),
        "--disable-autodetect".into(),
        // The card userland is static musl (no dynamic loader). libx265 is
        // C++: its .pc names -lc++; a static libc++ also needs these two.
        "--enable-static".into(),
        "--disable-shared".into(),
        // The card runtime (card/phix) is pulled in by reference to its
        // anchor symbol; FFmpeg's source never mentions it.
        format!(
            "--extra-ldflags=-static -Wl,--undefined=phix_anchor -L{}",
            phix.display()
        ),
        "--extra-libs=-lphix -lc++abi -lunwind".into(),
        "--disable-doc".into(),
        "--enable-pthreads".into(),
        "--enable-zlib".into(),
        // The card has none of FFmpeg's x86 SIMD instructions; inline
        // assembly also sits inside C functions (CABAC's uses CMOV).
        "--disable-asm".into(),
        "--disable-x86asm".into(),
        "--disable-inline-asm".into(),
        // libx265 is GPL, so the FFmpeg binary is too.
        "--enable-gpl".into(),
        "--enable-libx265".into(),
    ]
}

/// Run a command with the toolchain environment, output to a log file.
fn logged(cmd: &mut Command, env: &HashMap<String, String>, log: &Path, what: &str) -> Result<()> {
    let out = std::fs::File::create(log)?;
    let status = cmd
        .env_clear()
        .envs(env)
        .stdout(out.try_clone()?)
        .stderr(out)
        .status()
        .with_context(|| format!("running {what}"))?;
    if !status.success() {
        bail!("{what} failed; see {}", log.display());
    }
    Ok(())
}

/// Build the card workspace (`card/phix`, the runtime linked into FFmpeg
/// and x265, and `card/runner`, the transcode runner) for the stack's card
/// target and return the directory holding `libphix.a` and
/// `phifmpeg-card`. Always from clean: cargo's fingerprints do not cover
/// the patched LLVM library rustc loads (the stack's ADR 0007), so a
/// rebuilt LLVM would otherwise leave stale objects.
fn build_phix(
    stack: &Stack,
    repo: &Path,
    env: &HashMap<String, String>,
    log: &Path,
) -> Result<PathBuf> {
    let phi_root = PathBuf::from(&env["phi_root"]);
    let target = phi_root.join("toolchain/rust/x86_64-knc-linux-musl.json");
    let dylib = phi_root.join("toolchain/build/llvm-dylib/lib");
    if !dylib.join("libLLVM.so").exists() {
        bail!(
            "the stack's LLVM library for rustc is missing in {} (its toolchain/llvm, dylib variant)",
            dylib.display()
        );
    }
    let card = repo.join("card");
    let mut e = env.clone();
    e.insert("RUSTC_BOOTSTRAP".into(), "1".into());
    let ld = match env.get("LD_LIBRARY_PATH") {
        Some(old) if !old.is_empty() => format!("{}:{old}", dylib.display()),
        _ => dylib.display().to_string(),
    };
    e.insert("LD_LIBRARY_PATH".into(), ld);
    // The runner is a std program: the card target links it with knc-cc.
    e.insert(
        "CARGO_TARGET_X86_64_KNC_LINUX_MUSL_LINKER".into(),
        phi_root
            .join("toolchain/clang/knc-cc")
            .display()
            .to_string(),
    );
    println!("== building card/ (phix, phifmpeg-card) for the card");
    logged(
        Command::new("cargo").arg("clean").current_dir(&card),
        &e,
        log,
        "cargo clean in card/",
    )?;
    logged(
        Command::new("cargo")
            .args([
                "build",
                "-Zjson-target-spec",
                "-Zbuild-std=core,alloc,std,panic_abort",
                "--release",
                "--target",
            ])
            .arg(&target)
            .current_dir(&card),
        &e,
        log,
        "cargo build in card/",
    )?;
    let dir = card_target(repo);
    for built in [dir.join("libphix.a"), dir.join("phifmpeg-card")] {
        audit(stack, &built)?;
    }
    Ok(dir)
}

/// True when `out` exists and was modified before `input`.
fn older_than(out: &Path, input: &Path) -> bool {
    let m = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    matches!((m(out), m(input)), (Some(o), Some(i)) if o < i)
}

/// Reconfigure only when the argument list changed (kept in `flags_file`).
fn needs_configure(flags_file: &Path, args: &[String], marker: &Path) -> bool {
    let have = std::fs::read_to_string(flags_file).unwrap_or_default();
    have != args.join("\n") || !marker.is_file()
}

/// Build x265 for a variant and install it into its prefix.
fn build_x265(
    lay: &Layout,
    env: &HashMap<String, String>,
    v: Variant,
    phix: Option<&Path>,
) -> Result<()> {
    let dir = lay.x265_build(v.name());
    std::fs::create_dir_all(&dir)?;
    let args = x265_args(v, lay, env, phix);
    let flags_file = dir.join("phifmpeg.flags");
    if needs_configure(&flags_file, &args, &dir.join("build.ninja")) {
        println!("== configuring x265 {} in {}", v.name(), dir.display());
        logged(
            Command::new("cmake")
                .arg("-S")
                .arg(lay.x265_src().join("source"))
                .arg("-B")
                .arg(&dir)
                .args(&args),
            env,
            &dir.join("cmake.log"),
            "x265's cmake",
        )?;
        std::fs::write(&flags_file, args.join("\n"))?;
    }
    println!("== building x265 {}", v.name());
    logged(
        Command::new("ninja")
            .arg("-C")
            .arg(&dir)
            .arg(format!("-j{}", jobs()))
            .arg("install"),
        env,
        &dir.join("ninja.log"),
        "x265's ninja",
    )
}

/// Build x265 and FFmpeg for one variant (for the card, the card workspace
/// first) and audit every card binary. Returns the unstripped `ffmpeg_g`.
pub fn build(stack: &Stack, repo: &Path, lay: &Layout, pins: &Pins, v: Variant) -> Result<PathBuf> {
    pristine("ffmpeg", &pins.ffmpeg, &lay.ffmpeg_src())?;
    pristine("x265", &pins.x265, &lay.x265_src())?;
    if v == Variant::Host && !lay.tools().join("bin/nasm").is_file() {
        bail!(
            "nasm missing in {}; run `phifmpeg fetch`",
            lay.tools().display()
        );
    }
    // The card variant builds with the stack's environment (its PATH, and
    // CC=knc-cc, which CMake would honour); the host variant must not.
    let host = v == Variant::Host;
    let mut env: HashMap<String, String> = if host {
        std::env::vars().collect()
    } else {
        stack.toolchain_env()?
    };
    let phix = if host {
        None
    } else {
        Some(build_phix(
            stack,
            repo,
            &env,
            &lay.root.join("phix-build.log"),
        )?)
    };
    if let Some(dir) = &phix {
        // Neither FFmpeg's Makefile nor x265's Ninja files know that the
        // binaries depend on libphix.a (it comes in through link flags), so
        // a rebuilt library would leave stale binaries: remove the older
        // ones and let the build link them again.
        let lib = dir.join("libphix.a");
        let ff = lay.variant(v.name());
        let x = lay.x265_build(v.name());
        for out in ["ffmpeg_g", "ffmpeg", "ffprobe_g", "ffprobe"]
            .iter()
            .map(|n| ff.join(n))
            .chain([x.join("x265")])
        {
            if older_than(&out, &lib) {
                let _ = std::fs::remove_file(&out);
            }
        }
    }
    build_x265(lay, &env, v, phix.as_deref())?;
    env.insert(
        "PKG_CONFIG_LIBDIR".into(),
        lay.x265_prefix(v.name())
            .join("lib/pkgconfig")
            .display()
            .to_string(),
    );
    env.remove("PKG_CONFIG_PATH");

    let dir = lay.variant(v.name());
    std::fs::create_dir_all(&dir)?;
    let args = configure_args(lay, &dir, phix.as_deref());
    let flags_file = dir.join("phifmpeg.flags");
    if needs_configure(&flags_file, &args, &dir.join("ffbuild/config.mak")) {
        println!("== configuring ffmpeg {} in {}", v.name(), dir.display());
        logged(
            Command::new(lay.ffmpeg_src().join("configure"))
                .args(&args)
                .current_dir(&dir),
            &env,
            &dir.join("configure.log"),
            "FFmpeg's configure",
        )?;
        std::fs::write(&flags_file, args.join("\n"))?;
    }
    println!("== building ffmpeg {} with {} jobs", v.name(), jobs());
    logged(
        Command::new("make")
            .arg(format!("-j{}", jobs()))
            .current_dir(&dir),
        &env,
        &dir.join("make.log"),
        "make",
    )?;
    let bin = dir.join("ffmpeg_g");
    if host {
        return Ok(bin);
    }
    audit(stack, &bin)?;
    audit(stack, &lay.x265_build(v.name()).join("x265"))?;
    Ok(bin)
}

/// Run the stack's ISA audit on a card binary; any instruction the card
/// cannot execute fails the build. (The host variant is not audited: the
/// host executes everything.)
fn audit(stack: &Stack, bin: &Path) -> Result<()> {
    println!("== phi-isa-audit {}", bin.display());
    run(Command::new(stack.isa_audit()?).arg(bin))
}

#[cfg(test)]
mod tests {
    use super::{configure_args, x265_args, Variant};
    use crate::layout::Layout;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn lay() -> Layout {
        Layout {
            root: PathBuf::from("/b"),
        }
    }

    #[test]
    fn card_build_cross_compiles_without_assembly_and_links_phix() {
        let a = configure_args(&lay(), Path::new("/d"), Some(Path::new("/p")));
        for f in [
            "--cc=knc-cc",
            "--disable-asm",
            "--disable-x86asm",
            "--disable-inline-asm",
            "--extra-libs=-lphix -lc++abi -lunwind",
        ] {
            assert!(a.contains(&f.to_string()), "missing {f}");
        }
        assert!(a.iter().any(|x| x.contains("--undefined=phix_anchor")));
        assert!(!a.iter().any(|x| x.starts_with("--x86asmexe=")));
    }

    #[test]
    fn host_variant_is_native() {
        let a = configure_args(&lay(), Path::new("/d"), None);
        assert!(!a.iter().any(|x| x.contains("knc") || x.contains("cross")));
        assert!(!a.contains(&"--disable-inline-asm".to_string()));
        let x = x265_args(Variant::Host, &lay(), &HashMap::new(), None);
        assert!(x.contains(&"-DENABLE_ASSEMBLY=ON".to_string()));
        assert!(!x.iter().any(|a| a.contains("CMAKE_SYSTEM_NAME")));
    }

    #[test]
    fn older_than_compares_modification_times() {
        let d = std::env::temp_dir().join(format!("phifmpeg-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a"), d.join("b"));
        let t0 = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        for (p, dt) in [(&a, 0u64), (&b, 10)] {
            let f = std::fs::File::create(p).unwrap();
            f.set_modified(t0 + std::time::Duration::from_secs(dt))
                .unwrap();
        }
        assert!(super::older_than(&a, &b));
        assert!(!super::older_than(&b, &a));
        assert!(!super::older_than(&d.join("missing"), &b));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn x265_for_the_card_has_no_assembly_and_links_phix() {
        let x = x265_args(Variant::C, &lay(), &HashMap::new(), Some(Path::new("/p")));
        assert!(x.contains(&"-DENABLE_ASSEMBLY=OFF".to_string()));
        assert!(x.iter().any(|a| a.contains("--undefined=phix_anchor")));
    }
}
