//! `phix`: the card-side runtime linked into the unmodified FFmpeg and x265.
//!
//! It is a static library that the link flags pull in (`phifmpeg build`
//! passes `-Wl,--undefined=phix_anchor -lphix`); neither project's source
//! knows about it. A constructor in `.init_array` runs before `main` and
//! does nothing unless a `PHIX_*` variable asks for something, so a binary
//! with phix linked behaves exactly like one without.
//!
//! Features:
//! - `PHIX_PROF=<file>[,<microseconds>]`: sampling profiler ([`prof`]).

#![no_std]

mod prof;

/// Referenced by the link flags so the linker keeps this object, and with
/// it the constructor below (both are in the one object file the release
/// profile's `codegen-units = 1` produces).
#[no_mangle]
#[used]
#[allow(non_upper_case_globals)]
pub static phix_anchor: u8 = 0;

/// Runs before `main`, from `.init_array`.
extern "C" fn init() {
    prof::init();
}

#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = init;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // SAFETY: abort never returns and needs no state.
    unsafe { libc::abort() }
}
