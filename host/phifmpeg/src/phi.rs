//! The stack's `phi` command, as this repository uses it.
//!
//! Only the verbs the stack documents as its interface (`put`, `get`,
//! `run`, all with `-c N`) are used, through `<stack>/scripts/phi.sh`.
//! `run` passes the card command's exit status back. The stack's daemon
//! serves one session per card at a time (three concurrent 5 s `run`s take
//! 15.1 s), so a caller must never hold a long `run` while it needs `put`
//! or `get` on the same card; `transcode` goes through the card runner.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{bail, Context, Result};

use crate::stack::Stack;

/// Handle on the stack's `phi` command.
pub struct Phi {
    exe: PathBuf,
}

impl Phi {
    /// The `phi` command of a located stack.
    pub fn new(stack: &Stack) -> Phi {
        Phi {
            exe: stack.root.join("scripts/phi.sh"),
        }
    }

    fn cmd(&self, card: u32) -> Command {
        let mut c = Command::new(&self.exe);
        c.arg("-c").arg(card.to_string());
        c.stdin(Stdio::null());
        c
    }

    /// Copy a host file to the card.
    pub fn put(&self, card: u32, src: &Path, dst: &str) -> Result<()> {
        let out = self
            .cmd(card)
            .arg("put")
            .arg(src)
            .arg(dst)
            .output()
            .context("phi put")?;
        check(&out, &format!("phi -c {card} put {}", src.display()))
    }

    /// Copy a card file to the host.
    pub fn get(&self, card: u32, src: &str, dst: &Path) -> Result<()> {
        let out = self
            .cmd(card)
            .arg("get")
            .arg(src)
            .arg(dst)
            .output()
            .context("phi get")?;
        check(&out, &format!("phi -c {card} get {src}"))
    }

    /// Run a shell script on the card; returns its output whatever the
    /// exit status (the caller decides).
    pub fn sh(&self, card: u32, script: &str) -> Result<Output> {
        self.cmd(card)
            .args(["run", "sh", "-c", script])
            .output()
            .context("phi run")
    }

    /// Run an idempotent shell script on the card and fail on a non-zero
    /// exit. Tried three times: once (2026-09-27) a `chmod` through `run`
    /// returned 101 with no output and did not reproduce by hand.
    pub fn sh_ok(&self, card: u32, script: &str) -> Result<String> {
        let mut last = None;
        for _ in 0..3 {
            let out = self.sh(card, script)?;
            match check(&out, &format!("phi -c {card} run: {script}")) {
                Ok(()) => return Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Err(last.unwrap())
    }

    /// The card's `MemAvailable`, in bytes.
    pub fn mem_available(&self, card: u32) -> Result<u64> {
        let s = self.sh_ok(card, "grep MemAvailable /proc/meminfo")?;
        let kb: u64 = s
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
            .with_context(|| format!("card {card}: unexpected meminfo line {s:?}"))?;
        Ok(kb * 1024)
    }
}

fn check(out: &Output, what: &str) -> Result<()> {
    if !out.status.success() {
        bail!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
