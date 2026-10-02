//! `phifmpeg transcode`: one real-time HEVC transcode shared between the
//! cards and the host, the cards taking every segment they can finish in
//! time and the host doing the rest.
//!
//! The input is cut at keyframes into independent segments (stream copy,
//! no re-encode). Segments then arrive at the pace of the video itself, as
//! a live source would deliver them, and each has a deadline: its end time
//! in the video plus a fixed latency (`--latency`, like a broadcast delay).
//! A segment goes to an idle card slot if that card's measured speed, times
//! a safety factor, says it will finish before the deadline; otherwise to
//! the host. If a card segment has not finished by the last moment at which
//! the host could still make the deadline, the host encodes a backup copy;
//! whichever finishes first is kept and the other is stopped. So the cards
//! do as much as they can and real time never depends on them.
//!
//! The stack's control socket serves one session per card at a time
//! (measured: three 5 s commands took 15.1 s), so nothing here holds a
//! session while a segment encodes. Each card runs `phifmpeg-card` (the
//! card runner, `card/runner`), which encodes what appears in a job
//! directory on the card's disk, K at a time; one host thread per card
//! feeds it and collects from it with short visits.
//!
//! Every device runs the same pinned FFmpeg n9.0.2 and x265 4.2: the C
//! build on the cards, the SIMD build on the host (`phifmpeg build
//! --variant c` and `--variant host`). Segments are Matroska, joined with
//! FFmpeg's concat demuxer.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::layout::Layout;
use crate::phi::Phi;
use crate::util::{capture, sha256_file};

/// Where phifmpeg lives on a card's disk: the binaries under `bin/`, one
/// directory per transcode under `jobs/`.
const CARD_ROOT: &str = "/data/phifmpeg";
/// FFmpeg decode threads per card slot: 2 keeps a slot at 513 MiB peak
/// where 4 took 640 MiB, at the same speed.
const CARD_DECODE_THREADS: usize = 2;
/// x265 frame threads per card slot, fixed for the same reason.
const CARD_FRAME_THREADS: usize = 2;
/// FFmpeg decode threads per host slot.
const HOST_DECODE_THREADS: usize = 4;

/// Options for one transcode.
#[derive(clap::Args, Clone)]
pub struct Opts {
    /// Input video (any container FFmpeg reads; the first video stream is
    /// used, audio is copied through).
    pub input: PathBuf,
    /// Output file (Matroska).
    pub output: PathBuf,
    /// Cards to use.
    #[arg(long, value_delimiter = ',', default_value = "0,1")]
    pub cards: Vec<u32>,
    /// Encoder slots per card, each with `--card-pool` threads. Default: as
    /// many as the card's free memory holds at `--slot-mb` each, at most
    /// `--max-card-slots`; a card with room for none gets one small slot if
    /// `--small-slot-mb` fits.
    #[arg(long)]
    pub card_slots: Option<usize>,
    /// Memory one card slot needs, in MiB (measured peak 513 MiB for 1080p
    /// ultrafast with 2 decode threads, x265 pools=28, frame-threads=2).
    #[arg(long, default_value_t = 560)]
    pub slot_mb: u64,
    /// x265 pool threads per card slot (x265 breaks above 64; 28 is as
    /// fast as 57 and 130 MiB smaller).
    #[arg(long, default_value_t = 28)]
    pub card_pool: usize,
    /// A card that cannot hold one normal slot gets one small slot of this
    /// many MiB (measured peak 414 MiB with pools=14), if it fits.
    #[arg(long, default_value_t = 420)]
    pub small_slot_mb: u64,
    /// x265 pool threads in a small slot.
    #[arg(long, default_value_t = 14)]
    pub small_pool: usize,
    /// Most slots per card (5 measured best on card 0: 9.34 fps, against
    /// 8.22 with 4 larger slots; a sixth made segments too slow to finish).
    #[arg(long, default_value_t = 5)]
    pub max_card_slots: usize,
    /// Memory left free on each card, in MiB.
    #[arg(long, default_value_t = 200)]
    pub reserve_mb: u64,
    /// Starting estimate of one card slot's speed, frames per second; the
    /// measured speed replaces it after the first segment (5 slots on card
    /// 0 measured 1.9 each).
    #[arg(long, default_value_t = 1.9)]
    pub card_fps: f64,
    /// Host encoder slots.
    #[arg(long, default_value_t = 2)]
    pub host_slots: usize,
    /// x265 pool threads per host slot.
    #[arg(long, default_value_t = 8)]
    pub host_pool: usize,
    /// Starting estimate of one host slot's speed, frames per second.
    #[arg(long, default_value_t = 60.0)]
    pub host_fps: f64,
    /// x265 preset.
    #[arg(long, default_value = "ultrafast")]
    pub preset: String,
    /// x265 constant rate factor.
    #[arg(long, default_value_t = 28)]
    pub crf: u32,
    /// Target segment length in seconds (cuts happen at the next keyframe).
    #[arg(long, default_value_t = 2.0)]
    pub segment_seconds: f64,
    /// Real-time latency budget in seconds: a segment is due this long
    /// after its end time in the video.
    #[arg(long, default_value_t = 150.0)]
    pub latency: f64,
    /// Card estimates are multiplied by this before a deadline is judged
    /// (card encode times for equal segments spread from 57 to 85 s).
    #[arg(long, default_value_t = 1.2)]
    pub card_safety: f64,
    /// Seconds allowed for moving a segment to and from a card.
    #[arg(long, default_value_t = 3.0)]
    pub card_overhead: f64,
    /// Safety margin in seconds before every deadline.
    #[arg(long, default_value_t = 3.0)]
    pub margin: f64,
}

/// One input segment.
struct Seg {
    input: PathBuf,
    frames: u64,
    /// End time in the video, seconds from the first frame.
    end: f64,
}

/// The device that encoded a segment.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Device {
    Card(u32),
    /// A host slot.
    Host(usize),
}

impl Device {
    fn is_host(self) -> bool {
        matches!(self, Device::Host(_))
    }

    /// The report's per-device line: the host's slots count as one device.
    fn group(self) -> String {
        match self {
            Device::Card(c) => format!("card {c}"),
            Device::Host(_) => "host".to_string(),
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Device::Card(c) => write!(f, "card {c}"),
            Device::Host(s) => write!(f, "host {s}"),
        }
    }
}

/// A finished segment.
#[derive(Clone)]
struct Done {
    by: Device,
    secs: f64,
    at: f64,
    out: PathBuf,
}

/// What became of one card attempt at a segment.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The card's result was kept.
    Finished,
    /// The card finished, but after the host's backup had won.
    FinishedTooLate,
    /// Cancelled on the card because the host's backup won.
    HostWon,
    /// The encoder failed on the card.
    Failed,
    /// Encoded, but the result could not be fetched.
    FetchFailed,
}

impl Outcome {
    /// Every outcome, in the report's order.
    const ALL: [Outcome; 5] = [
        Outcome::Finished,
        Outcome::FinishedTooLate,
        Outcome::HostWon,
        Outcome::Failed,
        Outcome::FetchFailed,
    ];

    fn label(self) -> &'static str {
        match self {
            Outcome::Finished => "finished",
            Outcome::FinishedTooLate => "finished too late",
            Outcome::HostWon => "stopped, host won",
            Outcome::Failed => "failed",
            Outcome::FetchFailed => "fetch failed",
        }
    }
}

/// One card attempt at a segment, for the report.
struct Attempt {
    card: u32,
    secs: f64,
    outcome: Outcome,
}

#[derive(Default, Clone)]
struct State {
    /// The card it was given to, if any.
    card: Option<u32>,
    backup: bool,
    done: Option<Done>,
}

/// A card slot as the scheduler sees it: the runner on that card encodes
/// up to its slot count at once, first come first served. A slot holds one
/// segment at a time (placement takes idle slots only).
struct VSlot {
    card: u32,
    busy: bool,
}

/// What a card's agent thread is asked to do.
enum CardMsg {
    Submit(usize),
    Cancel(usize),
}

struct Shared {
    t0: Instant,
    opts: Opts,
    job: String,
    dir: PathBuf,
    segs: Vec<Seg>,
    state: Mutex<Vec<State>>,
    changed: Condvar,
    finished: AtomicBool,
    card_fps: Mutex<HashMap<u32, f64>>,
    host_fps: Mutex<f64>,
    attempts: Mutex<Vec<Attempt>>,
    slots: Mutex<Vec<VSlot>>,
    seg_slot: Mutex<Vec<Option<usize>>>,
    card_tx: Mutex<HashMap<u32, Sender<CardMsg>>>,
    host_ffmpeg: PathBuf,
    phi: Phi,
}

impl Shared {
    fn deadline(&self, i: usize) -> Instant {
        self.t0 + Duration::from_secs_f64(self.opts.latency + self.segs[i].end)
    }
    fn card_speed(&self, card: u32) -> f64 {
        *self
            .card_fps
            .lock()
            .unwrap()
            .get(&card)
            .unwrap_or(&self.opts.card_fps)
    }
    /// A finished card segment: the card's speed estimate learns from it.
    fn learn_card(&self, card: u32, fps: f64) {
        if let Some(e) = self.card_fps.lock().unwrap().get_mut(&card) {
            *e = learned(*e, fps);
        }
    }
    /// A card segment stopped unfinished after running this long: the card
    /// is at most this fast.
    fn learn_card_bound(&self, card: u32, fps: f64) {
        if let Some(e) = self.card_fps.lock().unwrap().get_mut(&card) {
            *e = bounded(*e, fps);
        }
    }
    fn host_est(&self, i: usize) -> f64 {
        self.segs[i].frames as f64 / *self.host_fps.lock().unwrap() + 1.0
    }
    fn is_done(&self, i: usize) -> bool {
        self.state.lock().unwrap()[i].done.is_some()
    }
    /// The card slot segment `i` held is free of it.
    fn release_slot(&self, i: usize) {
        let k = self.seg_slot.lock().unwrap()[i].take();
        if let Some(k) = k {
            self.slots.lock().unwrap()[k].busy = false;
        }
    }
    /// Record a finished segment; false if another device got there first.
    fn finish(&self, i: usize, by: Device, secs: f64, out: PathBuf) -> bool {
        {
            let mut st = self.state.lock().unwrap();
            if st[i].done.is_some() {
                return false;
            }
            st[i].done = Some(Done {
                by,
                secs,
                at: self.t0.elapsed().as_secs_f64(),
                out,
            });
        }
        self.release_slot(i);
        self.changed.notify_all();
        true
    }
    fn card_dir(&self) -> String {
        card_job_dir(&self.job)
    }
}

/// A speed estimate after a finished segment measured `fps`: halfway there.
fn learned(est: f64, fps: f64) -> f64 {
    0.5 * est + 0.5 * fps
}

/// A speed estimate after a segment stopped unfinished at `fps`, an upper
/// bound on the card's speed: 30 percent toward it, never onto it. Taking
/// the minimum (the first version) let one slow segment price a card out of
/// every later deadline, and then nothing could correct it.
fn bounded(est: f64, fps: f64) -> f64 {
    if fps < est {
        0.7 * est + 0.3 * fps
    } else {
        est
    }
}

/// How many encoder slots a card gets, each with how many x265 pool threads
/// and how much memory, from the MiB it has available: normal slots as far
/// as the memory beyond the reserve allows (at most `max_card_slots`), else
/// one small slot if that fits, else none. `--card-slots` overrides the
/// count.
fn slot_plan(o: &Opts, avail_mb: u64) -> (usize, usize, u64) {
    let room = avail_mb.saturating_sub(o.reserve_mb);
    let normal = ((room / o.slot_mb.max(1)) as usize).min(o.max_card_slots);
    match o.card_slots {
        Some(n) => (n, o.card_pool, o.slot_mb),
        None if normal > 0 => (normal, o.card_pool, o.slot_mb),
        None if room >= o.small_slot_mb => (1, o.small_pool, o.small_slot_mb),
        None => (0, 0, 0),
    }
}

/// The idle card slot that would finish a segment of `frames` soonest, if
/// that is by `deadline`: the slot's index and its expected finish.
/// `speed` is a card's estimated frames per second per slot. Idle slots
/// only: segments arrive every couple of seconds, so a slot that frees up
/// is refilled almost at once; queueing behind a busy slot (the first
/// version) turned the spread of card encode times (57 to 85 s for equal
/// segments) into late segments, cancelled work and a cascade.
fn place(
    slots: &[VSlot],
    frames: u64,
    speed: impl Fn(u32) -> f64,
    o: &Opts,
    now: Instant,
    deadline: Instant,
) -> Option<(usize, Instant)> {
    let mut best: Option<(usize, Instant)> = None;
    for (k, s) in slots.iter().enumerate() {
        if s.busy {
            continue;
        }
        let est = o.card_safety * frames as f64 / speed(s.card) + o.card_overhead;
        let finish = now + Duration::from_secs_f64(est);
        if best.is_none_or(|b| finish < b.1) {
            best = Some((k, finish));
        }
    }
    best.filter(|b| b.1 <= deadline)
}

/// One line of the runner's `done/` listing: `<file> <first line of it>`.
struct DoneLine<'a> {
    /// Segment number, from `seg<NNNNN>`.
    seg: usize,
    /// `ok`, `fail` or `cancelled`: the file's extension.
    kind: &'a str,
    /// The file's stem, `seg<NNNNN>`.
    name: &'a str,
    /// The file's first line.
    rest: &'a str,
    /// The seconds that line starts with (`ok`, `cancelled`); 0 if none.
    secs: f64,
}

fn parse_done_line(line: &str) -> Option<DoneLine<'_>> {
    let (file, rest) = line.split_once(' ').unwrap_or((line, ""));
    let (name, kind) = file.rsplit_once('.')?;
    let seg = name.strip_prefix("seg")?.parse().ok()?;
    let secs = rest
        .split_whitespace()
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    Some(DoneLine {
        seg,
        kind,
        name,
        rest,
        secs,
    })
}

/// A job's directory on a card's disk.
fn card_job_dir(job: &str) -> String {
    format!("{CARD_ROOT}/jobs/{job}")
}

/// x265's parameters for one slot: the pool size, frame threads where
/// they are fixed (the cards, for memory), and headers on every keyframe so
/// that segments join.
fn x265_params(pool: usize, frame_threads: Option<usize>) -> String {
    let mut p = format!("pools={pool}");
    if let Some(n) = frame_threads {
        p.push_str(&format!(":frame-threads={n}"));
    }
    p.push_str(":repeat-headers=1:log-level=error");
    p
}

/// Run one transcode.
pub fn transcode(lay: &Layout, phi: Phi, runner: &Path, opts: Opts) -> Result<()> {
    let host_ffmpeg = lay.variant("host").join("ffmpeg");
    let host_ffprobe = lay.variant("host").join("ffprobe");
    let card_ffmpeg = lay.variant("c").join("ffmpeg");
    for b in [
        host_ffmpeg.as_path(),
        host_ffprobe.as_path(),
        card_ffmpeg.as_path(),
        runner,
    ] {
        if !b.is_file() {
            bail!(
                "{} missing; run `phifmpeg build --variant host` and `--variant c`",
                b.display()
            );
        }
    }
    let job = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    let dir = lay.root.join("jobs").join(&job);
    std::fs::create_dir_all(dir.join("in"))?;
    std::fs::create_dir_all(dir.join("out"))?;

    // The input's frame rate, then segments cut at keyframes.
    let rate = capture(
        Command::new(&host_ffprobe)
            .args(["-v", "error", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=r_frame_rate", "-of", "csv=p=0"])
            .arg(&opts.input),
    )?;
    let fps = parse_rate(rate.trim())?;
    println!("== {} at {fps:.3} fps; job {job}", opts.input.display());
    let st = Command::new(&host_ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&opts.input)
        .args(["-map", "0:v:0", "-c", "copy", "-f", "segment"])
        .args(["-segment_time", &opts.segment_seconds.to_string()])
        .args(["-reset_timestamps", "1", "-segment_format", "matroska"])
        .arg(dir.join("in/seg%05d.mkv"))
        .status()?;
    if !st.success() {
        bail!("splitting {} failed", opts.input.display());
    }
    let mut segs = Vec::new();
    let mut t = 0.0;
    for i in 0.. {
        let p = dir.join(format!("in/seg{i:05}.mkv"));
        if !p.is_file() {
            break;
        }
        let n: u64 = capture(
            Command::new(&host_ffprobe)
                .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
                .args(["-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
                .arg(&p),
        )?
        .trim()
        .parse()
        .context("segment packet count")?;
        t += n as f64 / fps;
        segs.push(Seg {
            input: p,
            frames: n,
            end: t,
        });
    }
    let total: u64 = segs.iter().map(|s| s.frames).sum();
    println!(
        "== {} segments, {total} frames, {t:.1} s of video",
        segs.len()
    );

    // Per card: slots from free memory, binaries in place, runner started.
    let mut card_slots: Vec<(u32, usize)> = Vec::new();
    let cdir = card_job_dir(&job);
    for &c in &opts.cards {
        let avail = phi.mem_available(c)? / (1024 * 1024);
        let (n, pool, mb) = slot_plan(&opts, avail);
        println!("== card {c}: {avail} MiB available, {n} slot(s) of {mb} MiB, x265 pools={pool}");
        if n == 0 {
            continue;
        }
        let ffmpeg = format!("{CARD_ROOT}/bin/ffmpeg");
        let card_runner = format!("{CARD_ROOT}/bin/phifmpeg-card");
        install(&phi, c, &card_ffmpeg, &ffmpeg)?;
        install(&phi, c, runner, &card_runner)?;
        let encode = format!(
            "{ffmpeg} -nostdin -hide_banner -loglevel error -threads {CARD_DECODE_THREADS} -i {{in}} -c:v libx265 -preset {} -crf {} -x265-params {} -f matroska -y {{out}}",
            opts.preset,
            opts.crf,
            x265_params(pool, Some(CARD_FRAME_THREADS))
        );
        phi.sh_ok(
            c,
            &format!(
                "mkdir -p {cdir} && setsid {card_runner} serve {cdir} {n} {encode} > {cdir}/runner.log 2>&1 < /dev/null & sleep 0.3; cat {cdir}/runner.log"
            ),
        )?;
        card_slots.push((c, n));
    }

    let n = segs.len();
    let slots: Vec<VSlot> = card_slots
        .iter()
        .flat_map(|&(c, k)| {
            (0..k).map(move |_| VSlot {
                card: c,
                busy: false,
            })
        })
        .collect();
    let sh = Arc::new(Shared {
        t0: Instant::now(),
        card_fps: Mutex::new(card_slots.iter().map(|c| (c.0, opts.card_fps)).collect()),
        host_fps: Mutex::new(opts.host_fps),
        attempts: Mutex::new(Vec::new()),
        slots: Mutex::new(slots),
        seg_slot: Mutex::new(vec![None; n]),
        card_tx: Mutex::new(HashMap::new()),
        opts: opts.clone(),
        job: job.clone(),
        dir: dir.clone(),
        segs,
        state: Mutex::new(vec![State::default(); n]),
        changed: Condvar::new(),
        finished: AtomicBool::new(false),
        host_ffmpeg: host_ffmpeg.clone(),
        phi,
    });

    // Workers: host slots, one agent per card, the backup monitor.
    let (host_tx, host_rx) = mpsc::channel::<usize>();
    let (backup_tx, backup_rx) = mpsc::channel::<usize>();
    let host_rx = Arc::new(Mutex::new(host_rx));
    let backup_rx = Arc::new(Mutex::new(backup_rx));
    let mut handles = Vec::new();
    for h in 0..opts.host_slots {
        let (sh, a, b) = (sh.clone(), host_rx.clone(), backup_rx.clone());
        handles.push(thread::spawn(move || host_worker(h, sh, a, b)));
    }
    for &(c, _) in &card_slots {
        let (tx, rx) = mpsc::channel::<CardMsg>();
        sh.card_tx.lock().unwrap().insert(c, tx);
        let (sh2, btx) = (sh.clone(), backup_tx.clone());
        handles.push(thread::spawn(move || card_agent(c, sh2, rx, btx)));
    }
    {
        let (sh, btx) = (sh.clone(), backup_tx.clone());
        handles.push(thread::spawn(move || monitor(sh, btx)));
    }
    drop(backup_tx);

    // Segments arrive at the video's own pace: a live segment exists once
    // its last frame has arrived.
    for i in 0..n {
        let arrival = sh.t0 + Duration::from_secs_f64(sh.segs[i].end);
        let now = Instant::now();
        if arrival > now {
            thread::sleep(arrival - now);
        }
        let deadline = sh.deadline(i) - Duration::from_secs_f64(opts.margin);
        let now = Instant::now();
        let placed = {
            let mut slots = sh.slots.lock().unwrap();
            let frames = sh.segs[i].frames;
            let best = place(&slots, frames, |c| sh.card_speed(c), &opts, now, deadline);
            best.map(|(k, _)| {
                slots[k].busy = true;
                (k, slots[k].card)
            })
        };
        match placed {
            Some((k, card)) => {
                sh.seg_slot.lock().unwrap()[i] = Some(k);
                sh.state.lock().unwrap()[i].card = Some(card);
                if let Some(tx) = sh.card_tx.lock().unwrap().get(&card) {
                    tx.send(CardMsg::Submit(i)).ok();
                }
            }
            None => {
                host_tx.send(i).ok();
            }
        }
    }
    drop(host_tx);

    // Wait for every segment.
    {
        let mut st = sh.state.lock().unwrap();
        while st.iter().any(|s| s.done.is_none()) {
            st = sh.changed.wait(st).unwrap();
        }
    }
    let wall = sh.t0.elapsed().as_secs_f64();
    sh.finished.store(true, Ordering::SeqCst);
    sh.card_tx.lock().unwrap().clear();
    for h in handles {
        let _ = h.join();
    }

    report(&sh, wall, total, &card_slots)?;
    assemble(&sh, &opts, &host_ffprobe)?;
    Ok(())
}

/// Put `local` on the card at `remote` unless the same bytes are there.
fn install(phi: &Phi, card: u32, local: &Path, remote: &str) -> Result<()> {
    let want = sha256_file(local)?;
    let have = phi
        .sh_ok(card, &format!("sha256sum {remote} 2>/dev/null || true"))
        .unwrap_or_default();
    if !have.starts_with(&want) {
        println!("== card {card}: installing {remote}");
        let dir = remote.rsplit_once('/').map_or(".", |(d, _)| d);
        phi.sh_ok(card, &format!("mkdir -p {dir}"))?;
        // `put` keeps the local file mode (0755).
        phi.put(card, local, remote)?;
    }
    Ok(())
}

fn parse_rate(s: &str) -> Result<f64> {
    let (a, b) = s.split_once('/').unwrap_or((s, "1"));
    let (a, b): (f64, f64) = (a.parse()?, b.parse()?);
    if b == 0.0 || a <= 0.0 {
        bail!("unusable frame rate {s:?}");
    }
    Ok(a / b)
}

/// One card's agent: submit segments to the card's runner, pass on
/// cancellations, collect what it finished. Every visit to the card is a
/// short `phi` session, so the card's single session is never held long.
fn card_agent(card: u32, sh: Arc<Shared>, rx: Receiver<CardMsg>, backup: Sender<usize>) {
    let d = sh.card_dir();
    let mut outstanding: Vec<usize> = Vec::new();
    let mut last_poll = Instant::now();
    let mut open = true;
    loop {
        // Messages.
        while open {
            match rx.try_recv() {
                Ok(CardMsg::Submit(i)) => {
                    if sh.is_done(i) {
                        sh.release_slot(i);
                        continue;
                    }
                    let tmp = format!("{d}/in/.seg{i:05}.tmp");
                    let r = sh.phi.put(card, &sh.segs[i].input, &tmp).and_then(|_| {
                        sh.phi
                            .sh_ok(card, &format!("mv {tmp} {d}/in/seg{i:05}.mkv"))
                            .map(|_| ())
                    });
                    match r {
                        Ok(()) => outstanding.push(i),
                        Err(e) => {
                            eprintln!("   card {card}: could not submit segment {i}: {e:#}");
                            sh.release_slot(i);
                            backup.send(i).ok();
                        }
                    }
                }
                Ok(CardMsg::Cancel(i)) => {
                    if outstanding.contains(&i) {
                        let _ = sh.phi.sh_ok(card, &format!("touch {d}/cancel/seg{i:05}"));
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => open = false,
            }
        }
        // Results, about once a second.
        if !outstanding.is_empty() && last_poll.elapsed() >= Duration::from_secs(1) {
            last_poll = Instant::now();
            collect(card, &sh, &d, &mut outstanding, &backup);
        }
        if sh.finished.load(Ordering::SeqCst) {
            // Everything is done; stop whatever the card is still on.
            let mut script = String::new();
            for i in &outstanding {
                script.push_str(&format!("touch {d}/cancel/seg{i:05}; "));
            }
            script.push_str(&format!("touch {d}/stop"));
            let _ = sh.phi.sh_ok(card, &script);
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Fetch and record the runner's finished, failed and cancelled segments.
fn collect(card: u32, sh: &Shared, d: &str, outstanding: &mut Vec<usize>, backup: &Sender<usize>) {
    let listing = match sh.phi.sh_ok(
        card,
        &format!(
            "cd {d}/done && for f in *.ok *.fail *.cancelled; do [ -e \"$f\" ] && echo \"$f $(head -n1 \"$f\")\"; done; true"
        ),
    ) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut remove = Vec::new();
    for line in listing.lines() {
        let Some(DoneLine {
            seg: i,
            kind,
            name,
            rest,
            secs,
        }) = parse_done_line(line)
        else {
            continue;
        };
        if !outstanding.contains(&i) {
            continue;
        }
        let fps = if secs > 0.0 {
            sh.segs[i].frames as f64 / secs
        } else {
            0.0
        };
        let outcome = match kind {
            "ok" => {
                let out = sh.dir.join(format!("out/seg{i:05}.card{card}.mkv"));
                match sh.phi.get(card, &format!("{d}/done/{name}.mkv"), &out) {
                    Ok(()) => {
                        if sh.finish(i, Device::Card(card), secs, out) {
                            sh.learn_card(card, fps);
                            Outcome::Finished
                        } else {
                            // Finished after the host's backup: the card
                            // needed `secs`, so `fps` bounds its speed.
                            sh.learn_card_bound(card, fps);
                            Outcome::FinishedTooLate
                        }
                    }
                    Err(_) => {
                        if !sh.is_done(i) {
                            backup.send(i).ok();
                        }
                        Outcome::FetchFailed
                    }
                }
            }
            "cancelled" => {
                if secs > 0.0 {
                    sh.learn_card_bound(card, fps);
                }
                Outcome::HostWon
            }
            _ => {
                eprintln!("   card {card}: segment {i} failed on the card: {rest}");
                if !sh.is_done(i) {
                    sh.release_slot(i);
                    backup.send(i).ok();
                }
                Outcome::Failed
            }
        };
        sh.attempts.lock().unwrap().push(Attempt {
            card,
            secs,
            outcome,
        });
        outstanding.retain(|&x| x != i);
        remove.push(format!("{d}/done/{name}.*"));
    }
    if !remove.is_empty() {
        let _ = sh.phi.sh_ok(card, &format!("rm -f {}", remove.join(" ")));
    }
}

/// A host slot: backups first, then segments the cards could not take.
fn host_worker(
    slot: usize,
    sh: Arc<Shared>,
    normal: Arc<Mutex<Receiver<usize>>>,
    backup: Arc<Mutex<Receiver<usize>>>,
) {
    loop {
        let next = backup.lock().unwrap().try_recv().ok();
        let i = match next {
            Some(i) => i,
            None => match normal
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(i) => i,
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {
                    if sh.finished.load(Ordering::SeqCst) {
                        return;
                    }
                    continue;
                }
            },
        };
        if sh.is_done(i) {
            continue;
        }
        let t = Instant::now();
        let out = sh.dir.join(format!("out/seg{i:05}.host.mkv"));
        match host_encode(&sh, i, &out) {
            Ok(()) => {
                let secs = t.elapsed().as_secs_f64();
                if sh.finish(i, Device::Host(slot), secs, out) {
                    let fps = sh.segs[i].frames as f64 / secs;
                    let mut h = sh.host_fps.lock().unwrap();
                    *h = 0.5 * *h + 0.5 * fps;
                    drop(h);
                    // A backup that won: stop the card's copy.
                    let card = sh.state.lock().unwrap()[i].card;
                    if let Some(c) = card {
                        if let Some(tx) = sh.card_tx.lock().unwrap().get(&c) {
                            tx.send(CardMsg::Cancel(i)).ok();
                        }
                    }
                }
            }
            Err(e) => eprintln!("   host {slot}: segment {i} failed: {e:#}"),
        }
    }
}

fn host_encode(sh: &Shared, i: usize, out: &Path) -> Result<()> {
    let o = &sh.opts;
    let st = Command::new(&sh.host_ffmpeg)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .args(["-threads", &HOST_DECODE_THREADS.to_string(), "-i"])
        .arg(&sh.segs[i].input)
        .args([
            "-c:v",
            "libx265",
            "-preset",
            &o.preset,
            "-crf",
            &o.crf.to_string(),
        ])
        .args(["-x265-params", &x265_params(o.host_pool, None)])
        .args(["-f", "matroska", "-y"])
        .arg(out)
        .status()?;
    if !st.success() {
        bail!("host ffmpeg exited with {st}");
    }
    Ok(())
}

/// Hand a card segment to the host at the last moment the host can still
/// make its deadline.
fn monitor(sh: Arc<Shared>, backup: Sender<usize>) {
    while !sh.finished.load(Ordering::SeqCst) {
        let now = Instant::now();
        let mut send = Vec::new();
        {
            let mut st = sh.state.lock().unwrap();
            for (i, s) in st.iter_mut().enumerate() {
                if s.card.is_none() || s.done.is_some() || s.backup {
                    continue;
                }
                let last =
                    sh.deadline(i) - Duration::from_secs_f64(sh.host_est(i) + sh.opts.margin);
                if now >= last {
                    s.backup = true;
                    send.push(i);
                }
            }
        }
        for i in send {
            backup.send(i).ok();
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// Join the segments (FFmpeg's concat demuxer, stream copy) and mux them
/// with the input's audio. Each segment is Matroska, so it carries its own
/// timestamps (raw HEVC with B-frames has none to copy).
fn assemble(sh: &Shared, o: &Opts, ffprobe: &Path) -> Result<()> {
    let list = sh.dir.join("segments.txt");
    let mut text = String::new();
    for s in sh.state.lock().unwrap().iter() {
        let d = s.done.as_ref().unwrap();
        text.push_str(&format!("file '{}'\n", d.out.display()));
    }
    std::fs::write(&list, text)?;
    let st = Command::new(&sh.host_ffmpeg)
        .args([
            "-v", "error", "-nostdin", "-y", "-f", "concat", "-safe", "0",
        ])
        .arg("-i")
        .arg(&list)
        .arg("-i")
        .arg(&o.input)
        .args(["-map", "0:v", "-map", "1:a?", "-c", "copy"])
        .arg(&o.output)
        .status()?;
    if !st.success() {
        bail!("muxing {} failed", o.output.display());
    }
    let n = capture(
        Command::new(ffprobe)
            .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
            .args(["-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
            .arg(&o.output),
    )?;
    println!("== {}: {} frames", o.output.display(), n.trim());
    Ok(())
}

fn report(sh: &Shared, wall: f64, total: u64, cards: &[(u32, usize)]) -> Result<()> {
    let st = sh.state.lock().unwrap();
    let mut misses = 0;
    let mut backups = 0;
    let mut backup_wins = 0;
    let mut by_dev: Vec<(String, u64, usize)> = Vec::new();
    let mut lines = String::from(" seg  frames  device   secs  done at  slack\n");
    for (i, s) in st.iter().enumerate() {
        let d = s.done.as_ref().unwrap();
        let due = sh.opts.latency + sh.segs[i].end;
        let slack = due - d.at;
        if slack < 0.0 {
            misses += 1;
        }
        if s.backup {
            backups += 1;
            if d.by.is_host() {
                backup_wins += 1;
            }
        }
        let dev = d.by.group();
        match by_dev.iter_mut().find(|e| e.0 == dev) {
            Some(e) => {
                e.1 += sh.segs[i].frames;
                e.2 += 1;
            }
            None => by_dev.push((dev, sh.segs[i].frames, 1)),
        }
        // `to_string` so that the column width applies.
        lines.push_str(&format!(
            "{i:>4} {:>7}  {:<7} {:>5.1} {:>8.1} {:>6.1}\n",
            sh.segs[i].frames,
            d.by.to_string(),
            d.secs,
            d.at,
            slack
        ));
    }
    std::fs::write(sh.dir.join("segments.log"), &lines)?;
    let video = sh.segs.last().map(|s| s.end).unwrap_or(0.0);
    println!(
        "{total} frames, {video:.1} s of video, finished {wall:.1} s after the video began arriving (latency budget {:.0} s)",
        sh.opts.latency
    );
    println!("deadline misses: {misses}; host backups: {backups} ({backup_wins} won)");
    println!("card slots: {cards:?}");
    let att = sh.attempts.lock().unwrap();
    for &(c, _) in cards {
        for outcome in Outcome::ALL {
            let a: Vec<&Attempt> = att
                .iter()
                .filter(|a| a.card == c && a.outcome == outcome)
                .collect();
            if a.is_empty() {
                continue;
            }
            let secs: f64 = a.iter().map(|a| a.secs).sum();
            println!(
                "  card {c} attempts {}: {} ({secs:.0} slot-seconds)",
                outcome.label(),
                a.len()
            );
        }
    }
    drop(att);
    let mut est: Vec<(u32, f64)> = sh
        .card_fps
        .lock()
        .unwrap()
        .iter()
        .map(|(c, f)| (*c, *f))
        .collect();
    est.sort_by_key(|e| e.0);
    for (c, f) in est {
        println!("  card {c} speed estimate at the end: {f:.2} fps per slot");
    }
    by_dev.sort();
    for (dev, frames, segs) in &by_dev {
        println!(
            "  {dev:<7} {frames:>7} frames in {segs:>4} segments = {:>5.1}% of the video",
            *frames as f64 * 100.0 / total as f64
        );
    }
    println!(
        "per-segment table: {}",
        sh.dir.join("segments.log").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        bounded, card_job_dir, learned, parse_done_line, parse_rate, place, slot_plan, x265_params,
        Device, Opts, VSlot,
    };
    use std::time::{Duration, Instant};

    /// The options with their defaults, as `phifmpeg transcode in out`.
    fn opts() -> Opts {
        #[derive(clap::Parser)]
        struct P {
            #[command(flatten)]
            o: Opts,
        }
        <P as clap::Parser>::parse_from(["transcode", "in.mkv", "out.mkv"]).o
    }

    /// Slot sizing on the cards as they were measured: card 1 beside the
    /// AVX-512 worker (647 MiB) gets the small slot, card 0 with 3.6 GiB
    /// its five, and 5.1 GiB is still five (the cap).
    #[test]
    fn slots_from_free_memory() {
        let o = opts();
        assert_eq!(slot_plan(&o, 647), (1, o.small_pool, o.small_slot_mb));
        assert_eq!(slot_plan(&o, 3600), (5, o.card_pool, o.slot_mb));
        assert_eq!(slot_plan(&o, 5100), (5, o.card_pool, o.slot_mb));
        assert_eq!(slot_plan(&o, 1400), (2, o.card_pool, o.slot_mb));
        assert_eq!(slot_plan(&o, 500), (0, 0, 0));
        let forced = Opts {
            card_slots: Some(3),
            ..opts()
        };
        assert_eq!(slot_plan(&forced, 500), (3, o.card_pool, o.slot_mb));
    }

    #[test]
    fn placement_takes_the_fastest_idle_slot_that_makes_the_deadline() {
        let o = opts();
        let now = Instant::now();
        let slots = [
            VSlot {
                card: 0,
                busy: true,
            },
            VSlot {
                card: 0,
                busy: false,
            },
            VSlot {
                card: 1,
                busy: false,
            },
        ];
        // Card 1 is twice as fast: 120 frames at 4 fps x 1.2 safety + 3 s
        // overhead = 39 s.
        let speed = |c: u32| if c == 1 { 4.0 } else { 2.0 };
        let far = now + Duration::from_secs(150);
        let (k, finish) = place(&slots, 120, speed, &o, now, far).unwrap();
        assert_eq!(k, 2);
        assert!((finish - now).as_secs_f64() - 39.0 < 0.01);
        // Card 1 busy too: the idle card 0 slot, never the busy one.
        let mut slots = slots;
        slots[2].busy = true;
        assert_eq!(place(&slots, 120, speed, &o, now, far).unwrap().0, 1);
        // Too close to the deadline for any card: the host takes it.
        let soon = now + Duration::from_secs(30);
        assert!(place(&slots, 120, speed, &o, now, soon).is_none());
        slots[1].busy = true;
        assert!(place(&slots, 120, speed, &o, now, far).is_none());
    }

    #[test]
    fn learning_moves_halfway_and_bounds_softly() {
        assert_eq!(learned(2.0, 1.0), 1.5);
        assert!((bounded(2.0, 1.0) - 1.7).abs() < 1e-9);
        assert_eq!(
            bounded(2.0, 3.0),
            2.0,
            "a bound above the estimate says nothing"
        );
    }

    #[test]
    fn done_listing_lines() {
        let l = parse_done_line("seg00012.ok 61.250").unwrap();
        assert_eq!(
            (l.seg, l.kind, l.name, l.secs),
            (12, "ok", "seg00012", 61.25)
        );
        let l = parse_done_line("seg00003.cancelled 0").unwrap();
        assert_eq!((l.seg, l.kind, l.secs), (3, "cancelled", 0.0));
        let l = parse_done_line("seg00007.fail exit status: 137 after 40.1 s").unwrap();
        assert_eq!((l.seg, l.kind, l.secs), (7, "fail", 0.0));
        assert_eq!(l.rest, "exit status: 137 after 40.1 s");
        assert!(parse_done_line("runner.log 1").is_none());
        assert!(parse_done_line("").is_none());
    }

    #[test]
    fn frame_rates() {
        assert_eq!(parse_rate("60/1").unwrap(), 60.0);
        assert!((parse_rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(parse_rate("25").unwrap(), 25.0);
        assert!(parse_rate("0/0").is_err());
        assert!(parse_rate("60/0").is_err());
        assert!(parse_rate("x/1").is_err());
    }

    /// The card slot's parameters as measured (513 MiB peak); the host's
    /// leave frame threads to x265.
    #[test]
    fn x265_parameters() {
        assert_eq!(
            x265_params(28, Some(2)),
            "pools=28:frame-threads=2:repeat-headers=1:log-level=error"
        );
        assert_eq!(
            x265_params(8, None),
            "pools=8:repeat-headers=1:log-level=error"
        );
    }

    #[test]
    fn card_paths_and_device_names() {
        assert_eq!(card_job_dir("1727"), "/data/phifmpeg/jobs/1727");
        assert_eq!(Device::Card(1).to_string(), "card 1");
        assert_eq!(Device::Host(0).to_string(), "host 0");
        assert_eq!(Device::Host(1).group(), "host");
        assert_eq!(Device::Card(0).group(), "card 0");
        assert!(Device::Host(0).is_host() && !Device::Card(0).is_host());
    }
}
