//! `phifmpeg-card`: the card side of `phifmpeg transcode`.
//!
//! The stack's control socket serves one session per card at a time, so the
//! host cannot hold one open per running encoder. Instead this runner is
//! started once per job, detached, and works from a directory on the card's
//! disk; the host only makes short visits (a `put`, a rename, a listing, a
//! `get`).
//!
//! ```text
//! phifmpeg-card serve <dir> <slots> <program> [args...]
//! ```
//!
//! `args` may contain `{in}` and `{out}`, replaced per segment. Layout of
//! `<dir>`:
//!
//! | path | who writes | meaning |
//! | --- | --- | --- |
//! | `in/<name>.mkv` | host (renamed into place when complete) | a segment waiting |
//! | `run/<name>.mkv`, `run/<name>.out.mkv`, `run/<name>.log` | runner | a segment being encoded |
//! | `done/<name>.mkv` + `done/<name>.ok` | runner | finished; `.ok` holds the encode seconds, written last |
//! | `done/<name>.fail` | runner | the encoder failed; exit status and the log's tail |
//! | `done/<name>.cancelled` | runner | stopped on request; seconds it had run |
//! | `cancel/<name>` | host | stop this segment (running or waiting) |
//! | `stop` | host | exit once nothing is running or waiting |
//!
//! The `.ok`, `.fail` or `.cancelled` marker is always a segment's last
//! write, and appears whole (renamed from a dot name): once it exists, the
//! segment's `run/` files and `cancel/` request are gone.
//!
//! Segments are taken oldest name first, at most `<slots>` at a time. The
//! runner sets its own `oom_score_adj` to 1000 before starting anything, so
//! it and every encoder it starts go before any resident service when the
//! card runs out of memory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// One encoder process.
struct Running {
    name: String,
    child: Child,
    started: Instant,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 || args[1] != "serve" {
        eprintln!("usage: phifmpeg-card serve <dir> <slots> <program> [args...]");
        return ExitCode::from(2);
    }
    let dir = PathBuf::from(&args[2]);
    let slots: usize = match args[3].parse() {
        Ok(n) if n > 0 => n,
        _ => {
            eprintln!("phifmpeg-card: slots must be a positive number");
            return ExitCode::from(2);
        }
    };
    let program = args[4].clone();
    let template: Vec<String> = args[5..].to_vec();
    for d in ["in", "run", "done", "cancel"] {
        if let Err(e) = fs::create_dir_all(dir.join(d)) {
            eprintln!("phifmpeg-card: {}: {e}", dir.join(d).display());
            return ExitCode::from(1);
        }
    }
    // Inherited by every encoder.
    let _ = fs::write("/proc/self/oom_score_adj", "1000");
    eprintln!(
        "phifmpeg-card: serving {} with {slots} slot(s)",
        dir.display()
    );

    let mut running: Vec<Running> = Vec::new();
    loop {
        // Finished encoders.
        let mut i = 0;
        while i < running.len() {
            match running[i].child.try_wait() {
                Ok(Some(status)) => {
                    let r = running.swap_remove(i);
                    finish(&dir, &r, status.success(), &status.to_string());
                }
                Ok(None) => i += 1,
                Err(e) => {
                    let r = running.swap_remove(i);
                    finish(&dir, &r, false, &format!("wait failed: {e}"));
                }
            }
        }
        // Cancellations, running or waiting. The request is consumed and the
        // working files removed before the marker is published, so that a
        // reader who sees the marker sees nothing else left of the segment.
        for name in listing(&dir.join("cancel"), "") {
            let _ = fs::remove_file(dir.join("cancel").join(&name));
            if let Some(k) = running.iter().position(|r| r.name == name) {
                let mut r = running.swap_remove(k);
                let _ = r.child.kill();
                let _ = r.child.wait();
                let secs = r.started.elapsed().as_secs_f64();
                cleanup(&dir, &name);
                publish(&dir, &name, "cancelled", &format!("{secs:.3}\n"));
            } else if fs::remove_file(dir.join(format!("in/{name}.mkv"))).is_ok() {
                publish(&dir, &name, "cancelled", "0\n");
            }
        }
        // New work, oldest first.
        let waiting = listing(&dir.join("in"), ".mkv");
        for name in waiting.iter() {
            if running.len() >= slots {
                break;
            }
            let from = dir.join(format!("in/{name}.mkv"));
            let to = dir.join(format!("run/{name}.mkv"));
            if fs::rename(&from, &to).is_err() {
                continue;
            }
            match start(&dir, name, &program, &template) {
                Ok(child) => running.push(Running {
                    name: name.clone(),
                    child,
                    started: Instant::now(),
                }),
                Err(e) => {
                    cleanup(&dir, name);
                    publish(
                        &dir,
                        name,
                        "fail",
                        &format!("could not start {program}: {e}\n"),
                    );
                }
            }
        }
        if dir.join("stop").exists()
            && running.is_empty()
            && listing(&dir.join("in"), ".mkv").is_empty()
        {
            eprintln!("phifmpeg-card: stop");
            return ExitCode::SUCCESS;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Names in `d` ending in `suffix` (suffix removed), sorted; dot files
/// (partial uploads) excluded.
fn listing(d: &Path, suffix: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(d)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| !n.starts_with('.') && n.ends_with(suffix))
                .map(|n| n[..n.len() - suffix.len()].to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Start the encoder for `name`, its errors going to `run/<name>.log`.
fn start(dir: &Path, name: &str, program: &str, template: &[String]) -> std::io::Result<Child> {
    let input = dir.join(format!("run/{name}.mkv"));
    let output = dir.join(format!("run/{name}.out.mkv"));
    let log = fs::File::create(dir.join(format!("run/{name}.log")))?;
    let args: Vec<String> = template
        .iter()
        .map(|a| {
            a.replace("{in}", &input.to_string_lossy())
                .replace("{out}", &output.to_string_lossy())
        })
        .collect();
    Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
}

/// Publish a finished encoder's result: the output moved into `done/`, the
/// working files removed, and only then the marker the host polls for.
fn finish(dir: &Path, r: &Running, ok: bool, status: &str) {
    let secs = r.started.elapsed().as_secs_f64();
    let name = &r.name;
    let out = dir.join(format!("run/{name}.out.mkv"));
    let (kind, text) =
        if ok && out.is_file() && fs::rename(&out, dir.join(format!("done/{name}.mkv"))).is_ok() {
            ("ok", format!("{secs:.3}\n"))
        } else {
            // Read before cleanup removes it.
            let log = fs::read(dir.join(format!("run/{name}.log"))).unwrap_or_default();
            let tail = String::from_utf8_lossy(&log[log.len().saturating_sub(400)..]).into_owned();
            ("fail", format!("{status} after {secs:.1} s\n{tail}"))
        };
    cleanup(dir, name);
    publish(dir, name, kind, &text);
}

/// Write `done/<name>.<kind>`: always a segment's last write, so a reader
/// who sees it finds the segment's output (for `ok`) complete and its
/// working files and cancel request gone. Written under a dot name and
/// renamed, so it never appears empty (the host reads its first line).
fn publish(dir: &Path, name: &str, kind: &str, text: &str) {
    let tmp = dir.join(format!("done/.{name}.{kind}.tmp"));
    if fs::write(&tmp, text).is_ok() {
        let _ = fs::rename(&tmp, dir.join(format!("done/{name}.{kind}")));
    }
}

/// Remove a segment's working files.
fn cleanup(dir: &Path, name: &str) {
    for f in [
        format!("run/{name}.mkv"),
        format!("run/{name}.out.mkv"),
        format!("run/{name}.log"),
    ] {
        let _ = fs::remove_file(dir.join(f));
    }
}
