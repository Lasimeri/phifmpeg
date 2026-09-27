//! `phifmpeg build`: x265 and FFmpeg for the card, from the pristine trees,
//! with build-system flags as the only input.
//!
//! Two variants; each builds x265 into `<build>/prefix/<name>` and FFmpeg
//! (linked with that libx265) out of tree in `<build>/build/<name>`:
//!
//! - `c`: no assembly at all. Every instruction is one the card executes;
//!   the stack's `phi-isa-audit` must report zero illegal instructions or the
//!   build fails. This is the reference and the fallback.
//! - `asm`: FFmpeg's and x265's own x86 SIMD (SSE2 to AVX2) assembled with
//!   nasm. The card has none of those instructions, so they only run through
//!   the translator; the audit is reported, not enforced, because the hits
//!   are expected and must all sit inside the two projects' SIMD functions.
//!
//! FFmpeg's inline assembly is off in both: it is embedded in C functions
//! (CABAC uses CMOV there), where no function boundary exists to translate
//! at.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::fetch::{jobs, pristine};
use crate::layout::Layout;
use crate::pins::Pins;
use crate::run;
use crate::stack::Stack;

/// Which build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Variant {
    /// C only, audited clean.
    C,
    /// With FFmpeg's and x265's x86 assembly, for the translator.
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

/// CMake arguments for x265.
fn x265_args(v: Variant, lay: &Layout, env: &HashMap<String, String>, phix: &Path) -> Vec<String> {
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
    let mut a = vec![
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
        format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            lay.x265_prefix(v.name()).display()
        ),
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
    ];
    match v {
        Variant::C => a.push("-DENABLE_ASSEMBLY=OFF".into()),
        Variant::Asm => {
            a.push("-DENABLE_ASSEMBLY=ON".into());
            a.push(format!(
                "-DCMAKE_ASM_NASM_COMPILER={}",
                lay.tools().join("bin/nasm").display()
            ));
        }
    }
    a
}

/// FFmpeg configure arguments for a variant.
fn configure_args(v: Variant, lay: &Layout, dir: &Path, phix: &Path) -> Vec<String> {
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
        "--disable-inline-asm".into(),
        // libx265 is GPL, so the FFmpeg binary is too.
        "--enable-gpl".into(),
        "--enable-libx265".into(),
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

/// Build the card runtime (`card/phix`) for the stack's card target and
/// return the directory holding `libphix.a`. Always from clean: cargo's
/// fingerprints do not cover the patched LLVM library rustc loads (the
/// stack's ADR 0007), so a rebuilt LLVM would otherwise leave stale objects.
fn build_phix(stack: &Stack, repo: &Path, env: &HashMap<String, String>) -> Result<PathBuf> {
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
    println!("== building card/phix for the card");
    let log = lay_log(repo);
    logged(
        Command::new("cargo").arg("clean").current_dir(&card),
        &e,
        &log,
        "cargo clean in card/",
    )?;
    logged(
        Command::new("cargo")
            .args([
                "build",
                "-Zjson-target-spec",
                "-Zbuild-std=core",
                "--release",
                "--target",
            ])
            .arg(&target)
            .current_dir(&card),
        &e,
        &log,
        "cargo build in card/",
    )?;
    let dir = card.join("target/x86_64-knc-linux-musl/release");
    let lib = dir.join("libphix.a");
    println!("== phi-isa-audit {}", lib.display());
    run(Command::new(stack.isa_audit()?).arg(&lib))?;
    Ok(dir)
}

/// Log file for the card crate build.
fn lay_log(repo: &Path) -> PathBuf {
    repo.join("card/target/phifmpeg-build.log")
}

/// Reconfigure only when the argument list changed (kept in `flags_file`).
fn needs_configure(flags_file: &Path, args: &[String], marker: &Path) -> bool {
    let have = std::fs::read_to_string(flags_file).unwrap_or_default();
    have != args.join("\n") || !marker.is_file()
}

/// Build x265 for a variant and install it into its prefix.
fn build_x265(lay: &Layout, env: &HashMap<String, String>, v: Variant, phix: &Path) -> Result<()> {
    let dir = lay.variant(&format!("x265-{}", v.name()));
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

/// Build x265 and FFmpeg for one variant and audit the result. Returns the
/// unstripped `ffmpeg_g`.
pub fn build(stack: &Stack, repo: &Path, lay: &Layout, pins: &Pins, v: Variant) -> Result<PathBuf> {
    pristine("ffmpeg", &pins.ffmpeg, &lay.ffmpeg_src())?;
    pristine("x265", &pins.x265, &lay.x265_src())?;
    if v == Variant::Asm && !lay.tools().join("bin/nasm").is_file() {
        bail!(
            "nasm missing in {}; run `phifmpeg fetch`",
            lay.tools().display()
        );
    }
    let mut env = stack.toolchain_env()?;
    let phix = build_phix(stack, repo, &env)?;
    build_x265(lay, &env, v, &phix)?;
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
    let args = configure_args(v, lay, &dir, &phix);
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
    audit(stack, &bin, v)?;
    audit(
        stack,
        &lay.variant(&format!("x265-{}", v.name())).join("x265"),
        v,
    )?;
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
            // Hits are expected: the SIMD functions. Report them.
            let _ = cmd.status();
            Ok(())
        }
    }
}
