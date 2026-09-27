//! `phifmpeg`: an unmodified FFmpeg on the Xeon Phi 3120 cards.
//!
//! This binary is everything around FFmpeg that runs on the host: it finds
//! the stack, fetches the pinned sources, and builds FFmpeg for the card
//! with configure flags alone. FFmpeg's source is never edited; `fetch` and
//! every `build` check that the tree is exactly the pinned commit.

mod fetch;
mod ffbuild;
mod layout;
mod pins;
mod stack;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use ffbuild::Variant;
use layout::Layout;
use pins::Pins;
use stack::Stack;

#[derive(Parser)]
#[command(version, about = "An unmodified FFmpeg on the Xeon Phi 3120 cards")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show where the stack, its toolchain and the build root are.
    Stack,
    /// Fetch the pinned FFmpeg and build the pinned nasm.
    Fetch,
    /// Build FFmpeg for the card (configure flags only) and audit it.
    Build {
        /// `c` (no assembly, audited clean) or `asm` (FFmpeg's x86 SIMD in).
        #[arg(long, value_enum, default_value = "c")]
        variant: Variant,
    },
}

/// This repository's root (the crate is at host/phifmpeg).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// Run a command, failing on a non-zero exit.
pub(crate) fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("running {cmd:?}: {e}"))?;
    if !status.success() {
        bail!("{cmd:?} exited with {status}");
    }
    Ok(())
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("phifmpeg: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let repo = repo_root();
    let lay = Layout::new(&repo)?;
    let pins = Pins::load(&repo)?;
    match cli.cmd {
        Cmd::Stack => {
            let s = Stack::find(&repo)?;
            let env = s.toolchain_env()?;
            println!("stack       {} (found by {})", s.root.display(), s.found_by);
            println!("phi_root    {}", env["phi_root"]);
            println!("PHI_LLVM    {}", env["PHI_LLVM"]);
            println!("PHI_SYSROOT {}", env["PHI_SYSROOT"]);
            println!("isa audit   {}", s.isa_audit()?.display());
            println!("build root  {}", lay.root.display());
            println!("ffmpeg      {} at {}", pins.ffmpeg.tag, pins.ffmpeg.commit);
        }
        Cmd::Fetch => fetch::fetch(&lay, &pins)?,
        Cmd::Build { variant } => {
            let s = Stack::find(&repo)?;
            let bin = ffbuild::build(&s, &lay, &pins, variant)?;
            println!("== built {}", bin.display());
        }
    }
    Ok(())
}
