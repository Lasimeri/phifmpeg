//! `phifmpeg fetch`: the pinned FFmpeg and x265 checkouts and a host nasm.
//!
//! FFmpeg and x265 are cloned at their pinned tags and must peel to the
//! pinned commits with no local changes; [`pristine`] is also the guard
//! every build runs, so an edited tree can never be built by accident.
//! nasm assembles FFmpeg's and x265's own x86 sources for the `host`
//! variant; it is built from a SHA-256-checked tarball into the build root,
//! so nothing is installed on the host.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use crate::layout::Layout;
use crate::pins::{GitPin, Pins};
use crate::util::{capture, jobs, run, sha256_file};

/// Fetch everything `pins.toml` names.
pub fn fetch(lay: &Layout, pins: &Pins) -> Result<()> {
    fetch_git("ffmpeg", &pins.ffmpeg, &lay.ffmpeg_src())?;
    fetch_git("x265", &pins.x265, &lay.x265_src())?;
    fetch_nasm(lay, pins)?;
    Ok(())
}

fn fetch_git(name: &str, pin: &GitPin, dir: &Path) -> Result<()> {
    if !dir.join(".git").is_dir() {
        std::fs::create_dir_all(dir.parent().unwrap())?;
        println!("== cloning {} at {}", pin.repo, pin.tag);
        run(Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--branch",
                &pin.tag,
                "-c",
                "advice.detachedHead=false",
            ])
            .arg(&pin.repo)
            .arg(dir))?;
    }
    pristine(name, pin, dir)?;
    println!("== {name} {} at {} (pristine)", pin.tag, &pin.commit[..12]);
    Ok(())
}

/// The checkout is at the pinned commit and has no changes, tracked or not.
pub fn pristine(name: &str, pin: &GitPin, dir: &Path) -> Result<()> {
    let head = capture(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"]),
    )?;
    if head.trim() != pin.commit {
        bail!(
            "{}: HEAD is {} but pins.toml says {}",
            dir.display(),
            head.trim(),
            pin.commit
        );
    }
    let status = capture(
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["status", "--porcelain"]),
    )?;
    if !status.trim().is_empty() {
        bail!(
            "{}: the {name} tree has local changes; phifmpeg builds {name} unmodified:\n{}",
            dir.display(),
            status
        );
    }
    Ok(())
}

fn fetch_nasm(lay: &Layout, pins: &Pins) -> Result<()> {
    let nasm = lay.tools().join("bin/nasm");
    if nasm.is_file() {
        let v = capture(Command::new(&nasm).arg("-v"))?;
        if v.contains(&format!("version {} ", pins.nasm.version)) {
            println!("== nasm {} present", pins.nasm.version);
            return Ok(());
        }
    }
    let name = pins.nasm.url.rsplit('/').next().unwrap();
    let tarball = lay.downloads().join(name);
    std::fs::create_dir_all(lay.downloads())?;
    if !tarball.is_file() {
        println!("== downloading {}", pins.nasm.url);
        let part = tarball.with_extension("part");
        run(Command::new("curl")
            .args(["-fL", "--retry", "3", "-o"])
            .arg(&part)
            .arg(&pins.nasm.url))?;
        std::fs::rename(&part, &tarball)?;
    }
    let got = sha256_file(&tarball)?;
    if got != pins.nasm.sha256 {
        bail!(
            "{}: SHA-256 {} but pins.toml says {}",
            tarball.display(),
            got,
            pins.nasm.sha256
        );
    }
    let src = lay.src(&format!("nasm-{}", pins.nasm.version));
    if src.exists() {
        std::fs::remove_dir_all(&src)?;
    }
    std::fs::create_dir_all(src.parent().unwrap())?;
    run(Command::new("tar")
        .arg("-xJf")
        .arg(&tarball)
        .arg("-C")
        .arg(src.parent().unwrap()))?;
    println!("== building nasm {}", pins.nasm.version);
    let prefix = format!("--prefix={}", lay.tools().display());
    run(Command::new("./configure").arg(&prefix).current_dir(&src))?;
    run(Command::new("make")
        .arg(format!("-j{}", jobs()))
        .current_dir(&src))?;
    run(Command::new("make").arg("install").current_dir(&src))?;
    Ok(())
}
