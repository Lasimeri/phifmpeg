//! Helpers every command shares: running programs, hashing files, and the
//! parallelism of host builds.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// Run a command with inherited stdio, failing on a non-zero exit.
pub fn run(cmd: &mut Command) -> Result<()> {
    let status = cmd.status().with_context(|| format!("running {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} exited with {status}");
    }
    Ok(())
}

/// Run a command and return its stdout; fail on a non-zero exit with its
/// stderr in the message.
pub fn capture(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Hex SHA-256 of a file.
pub fn sha256_file(p: &Path) -> Result<String> {
    let mut f = std::fs::File::open(p).with_context(|| format!("opening {}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Parallel jobs for host builds: `PHIFMPEG_JOBS`, else the CPU count.
pub fn jobs() -> usize {
    std::env::var("PHIFMPEG_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        })
}

#[cfg(test)]
mod tests {
    use super::{capture, run, sha256_file};
    use std::process::Command;

    #[test]
    fn run_and_capture_report_exit_status() {
        assert!(run(&mut Command::new("true")).is_ok());
        assert!(run(&mut Command::new("false")).is_err());
        assert_eq!(
            capture(Command::new("sh").args(["-c", "printf hi"])).unwrap(),
            "hi"
        );
        let e = capture(Command::new("sh").args(["-c", "echo oops >&2; exit 3"]))
            .unwrap_err()
            .to_string();
        assert!(e.contains("oops"), "{e}");
    }

    #[test]
    fn sha256_of_a_known_file() {
        let p = std::env::temp_dir().join(format!("phifmpeg-sha-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_file(&p).unwrap();
    }
}
