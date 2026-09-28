//! `phifmpeg`: an unmodified FFmpeg (with an unmodified x265) on the Xeon
//! Phi 3120 cards, the cards working as co-processors beside the host.
//!
//! This binary is everything around FFmpeg that runs on the host: it finds
//! the stack, fetches the pinned sources, builds x265 and FFmpeg with build
//! flags alone (C for the cards, SIMD for the host) together with the card
//! workspace, runs real-time transcodes split between the cards and the
//! host, and reads the card profiler's samples. Neither upstream source is
//! ever edited; `fetch` and every `build` check that each tree is exactly
//! its pinned commit.

mod fetch;
mod ffbuild;
mod layout;
mod phi;
mod pins;
mod prof;
mod stack;
mod transcode;

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

use ffbuild::Variant;
use layout::Layout;
use pins::Pins;
use stack::Stack;

#[derive(Parser)]
#[command(
    version,
    about = "An unmodified FFmpeg and x265 on the Xeon Phi 3120 cards, as co-processors beside the host"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show where the stack, its toolchain and the build root are.
    Stack,
    /// Fetch the pinned FFmpeg and x265 and build the pinned nasm.
    Fetch,
    /// Real-time HEVC transcode shared between the cards and the host.
    Transcode(transcode::Opts),
    /// Map card profiler samples (PHIX_PROF) to functions.
    Prof {
        /// The unstripped binary that ran (ffmpeg_g, x265).
        elf: PathBuf,
        /// The samples file PHIX_PROF wrote.
        samples: PathBuf,
        /// How many functions to list.
        #[arg(long, default_value_t = 30)]
        top: usize,
    },
    /// Build x265 and FFmpeg (build flags only): for the card, with the card
    /// workspace, audited; or for the host.
    Build {
        /// `c` (card, no assembly, audited clean), `asm` (card, both projects'
        /// x86 SIMD in, for the vector-unit work) or `host` (native, all SIMD).
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
        Cmd::Transcode(o) => {
            let s = Stack::find(&repo)?;
            let runner = repo.join("card/target/x86_64-knc-linux-musl/release/phifmpeg-card");
            transcode::transcode(&lay, phi::Phi::new(&s), &runner, o)?;
        }
        Cmd::Prof { elf, samples, top } => prof::report(&elf, &samples, top)?,
        Cmd::Build { variant } => {
            let s = Stack::find(&repo)?;
            let bin = ffbuild::build(&s, &repo, &lay, &pins, variant)?;
            println!("== built {}", bin.display());
        }
    }
    Ok(())
}
