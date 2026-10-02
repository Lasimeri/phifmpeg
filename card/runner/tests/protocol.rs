//! The runner's job-directory protocol, exercised on the host with `sh` as
//! the encoder. Every scenario the host side (`transcode.rs`) relies on:
//! dot files are ignored, a segment is claimed and published with its
//! seconds, a failed encoder leaves a `.fail` with its status and log, a
//! running or a waiting segment is cancelled, and `stop` ends the runner
//! once it is idle.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// One runner process on a fresh job directory; killed when dropped.
struct Runner {
    dir: PathBuf,
    child: Child,
}

impl Runner {
    /// Start `phifmpeg-card serve <dir> <slots> sh -c <script>`; the script
    /// sees `{in}` and `{out}` replaced.
    fn start(name: &str, slots: usize, script: &str) -> Runner {
        let dir =
            std::env::temp_dir().join(format!("phifmpeg-card-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_phifmpeg-card"))
            .arg("serve")
            .arg(&dir)
            .arg(slots.to_string())
            .args(["sh", "-c", script])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let r = Runner { dir, child };
        // The runner creates in/, run/, done/, cancel/ first thing.
        r.wait_for(&r.dir.join("cancel"));
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.join(p)
    }

    /// Put a segment in place the way the host does: under a dot name,
    /// then renamed.
    fn submit(&self, name: &str, bytes: &[u8]) {
        let tmp = self.path(&format!("in/.{name}.tmp"));
        fs::write(&tmp, bytes).unwrap();
        fs::rename(tmp, self.path(&format!("in/{name}.mkv"))).unwrap();
    }

    fn touch(&self, p: &str) {
        fs::write(self.path(p), "").unwrap();
    }

    /// Wait up to 10 s for a path to exist.
    fn wait_for(&self, p: &Path) {
        let t = Instant::now();
        while !p.exists() {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "{} did not appear",
                p.display()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn names(&self, sub: &str) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.path(sub))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    /// Wait up to 10 s for the runner to exit; its exit code.
    fn wait_exit(&mut self) -> i32 {
        let t = Instant::now();
        loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                return st.code().unwrap_or(-1);
            }
            assert!(t.elapsed() < Duration::from_secs(10), "runner did not exit");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn claims_encodes_publishes_and_stops() {
    let mut r = Runner::start("ok", 1, "cp {in} {out}");
    // A partial upload (dot name) is not touched.
    fs::write(r.path("in/.seg00099.tmp"), b"partial").unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(r.path("in/.seg00099.tmp").exists());
    assert!(r.names("run").is_empty() && r.names("done").is_empty());

    r.submit("seg00000", b"segment zero");
    r.wait_for(&r.path("done/seg00000.ok"));
    assert_eq!(
        fs::read(r.path("done/seg00000.mkv")).unwrap(),
        b"segment zero"
    );
    let secs: f64 = fs::read_to_string(r.path("done/seg00000.ok"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(secs >= 0.0 && secs < 10.0, "{secs}");
    assert!(r.names("run").is_empty(), "working files cleaned");
    assert_eq!(
        r.names("in"),
        vec![".seg00099.tmp"],
        "partial still ignored"
    );

    // `stop` with nothing waiting or running: exit 0.
    r.touch("stop");
    assert_eq!(r.wait_exit(), 0);
}

#[test]
fn failed_encoder_reports_status_and_log() {
    let r = Runner::start("fail", 1, "echo bad input >&2; exit 7");
    r.submit("seg00003", b"x");
    r.wait_for(&r.path("done/seg00003.fail"));
    let fail = fs::read_to_string(r.path("done/seg00003.fail")).unwrap();
    assert!(fail.contains("exit status: 7"), "{fail}");
    assert!(fail.contains("bad input"), "{fail}");
    assert!(!r.path("done/seg00003.mkv").exists());
    assert!(!r.path("done/seg00003.ok").exists());
    assert!(r.names("run").is_empty());
}

#[test]
fn cancels_a_running_and_a_waiting_segment() {
    // One slot: the first segment runs, the second waits.
    let mut r = Runner::start("cancel", 1, "sleep 30");
    r.submit("seg00010", b"a");
    r.submit("seg00011", b"b");
    r.wait_for(&r.path("run/seg00010.mkv"));
    thread::sleep(Duration::from_millis(100));
    assert_eq!(r.names("in"), vec!["seg00011.mkv"], "second one waits");

    r.touch("cancel/seg00011");
    r.wait_for(&r.path("done/seg00011.cancelled"));
    assert_eq!(
        fs::read_to_string(r.path("done/seg00011.cancelled")).unwrap(),
        "0\n",
        "never ran"
    );
    assert!(r.names("in").is_empty());

    r.touch("cancel/seg00010");
    r.wait_for(&r.path("done/seg00010.cancelled"));
    let secs: f64 = fs::read_to_string(r.path("done/seg00010.cancelled"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(secs > 0.0, "ran for a while");
    assert!(r.names("run").is_empty(), "killed and cleaned");
    assert!(r.names("cancel").is_empty(), "requests consumed");

    // Stop honoured now that it is idle.
    r.touch("stop");
    assert_eq!(r.wait_exit(), 0);
}

#[test]
fn stop_waits_for_running_work() {
    let mut r = Runner::start("stop", 2, "sleep 0.5; cp {in} {out}");
    r.submit("seg00020", b"p");
    r.submit("seg00021", b"q");
    r.touch("stop");
    assert_eq!(r.wait_exit(), 0);
    assert_eq!(
        r.names("done"),
        vec!["seg00020.mkv", "seg00020.ok", "seg00021.mkv", "seg00021.ok"]
    );
}
