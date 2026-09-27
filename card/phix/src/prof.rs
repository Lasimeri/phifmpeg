//! Sampling profiler: where the card spends its time.
//!
//! `PHIX_PROF=<file>[,<microseconds>]` arms `ITIMER_PROF`, which counts the
//! CPU time of the whole process (every thread) and raises `SIGPROF` each
//! time the interval is used up. The handler records the interrupted
//! instruction pointer; at exit the samples are written to `<file>` as
//! little-endian `u64`s, and `phifmpeg prof` maps them to functions using
//! the binary's symbol table on the host.
//!
//! The interval defaults to 10 ms of process CPU time: at 100 busy threads
//! that is about 10,000 samples a second, and the buffer holds 4 Mi
//! samples (32 MiB of address space, reserved but only touched as used).
//!
//! The handler only does an atomic increment and one store, both safe in a
//! signal handler.

use core::ffi::{c_int, c_void};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Samples the buffer can hold.
const CAP: usize = 1 << 22;
/// Default interval in microseconds of process CPU time.
const DEFAULT_US: i64 = 10_000;

static BUF: AtomicPtr<u64> = AtomicPtr::new(core::ptr::null_mut());
static COUNT: AtomicUsize = AtomicUsize::new(0);
/// Output path, NUL-terminated; written once in `init` before the timer
/// is armed, read once at exit.
static mut PATH: [u8; 512] = [0; 512];

/// Arm the profiler if `PHIX_PROF` is set.
pub fn init() {
    // SAFETY: getenv before main, no other thread exists yet.
    let v = unsafe { libc::getenv(c"PHIX_PROF".as_ptr()) };
    if v.is_null() {
        return;
    }
    // SAFETY: getenv returned a NUL-terminated string.
    let spec = unsafe { core::ffi::CStr::from_ptr(v) }.to_bytes();
    let (path, us) = match spec.iter().position(|&b| b == b',') {
        Some(i) => (&spec[..i], parse_us(&spec[i + 1..])),
        None => (spec, DEFAULT_US),
    };
    if path.is_empty() || path.len() >= 512 {
        return;
    }
    // SAFETY: single-threaded here (constructor), and nothing reads PATH
    // until exit.
    unsafe {
        let p = (&raw mut PATH) as *mut u8;
        core::ptr::copy_nonoverlapping(path.as_ptr(), p, path.len());
        *p.add(path.len()) = 0;
    }

    // SAFETY: plain syscalls with valid arguments; the mapping is private
    // and anonymous, reserved without swap.
    unsafe {
        let buf = libc::mmap(
            core::ptr::null_mut(),
            CAP * 8,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
            -1,
            0,
        );
        if buf == libc::MAP_FAILED {
            return;
        }
        BUF.store(buf as *mut u64, Ordering::Release);

        let mut sa: libc::sigaction = core::mem::zeroed();
        sa.sa_sigaction = on_prof as *const () as usize;
        sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        if libc::sigaction(libc::SIGPROF, &sa, core::ptr::null_mut()) != 0 {
            return;
        }
        libc::atexit(write_out);

        let tv = libc::timeval {
            tv_sec: us / 1_000_000,
            tv_usec: us % 1_000_000,
        };
        let it = libc::itimerval {
            it_interval: tv,
            it_value: tv,
        };
        libc::setitimer(libc::ITIMER_PROF, &it, core::ptr::null_mut());
    }
}

/// Decimal microseconds, clamped to a sane range; bad input means default.
fn parse_us(s: &[u8]) -> i64 {
    let mut n: i64 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return DEFAULT_US;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as i64);
    }
    n.clamp(100, 1_000_000)
}

/// SIGPROF: record the interrupted instruction pointer.
extern "C" fn on_prof(_sig: c_int, _info: *mut libc::siginfo_t, ctx: *mut c_void) {
    let buf = BUF.load(Ordering::Acquire);
    if buf.is_null() || ctx.is_null() {
        return;
    }
    let i = COUNT.fetch_add(1, Ordering::Relaxed);
    if i < CAP {
        // SAFETY: the kernel passes a valid ucontext_t; i < CAP keeps the
        // store inside the mapping.
        unsafe {
            let uc = ctx as *const libc::ucontext_t;
            let rip = (*uc).uc_mcontext.gregs[libc::REG_RIP as usize] as u64;
            *buf.add(i) = rip;
        }
    }
}

/// At exit: stop the timer and write the samples.
extern "C" fn write_out() {
    // SAFETY: plain syscalls; PATH was set in init and is not written again.
    unsafe {
        let zero: libc::itimerval = core::mem::zeroed();
        libc::setitimer(libc::ITIMER_PROF, &zero, core::ptr::null_mut());
        let buf = BUF.load(Ordering::Acquire);
        if buf.is_null() {
            return;
        }
        let n = COUNT.load(Ordering::Relaxed).min(CAP);
        let path = (&raw const PATH) as *const libc::c_char;
        let fd = libc::open(path, libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC, 0o644);
        if fd < 0 {
            return;
        }
        let mut p = buf as *const u8;
        let mut left = n * 8;
        while left > 0 {
            let w = libc::write(fd, p as *const c_void, left);
            if w <= 0 {
                break;
            }
            p = p.add(w as usize);
            left -= w as usize;
        }
        libc::close(fd);
    }
}
