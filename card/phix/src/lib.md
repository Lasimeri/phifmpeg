# lib.rs

The `phix` static library: the card-side runtime that rides inside the
unmodified FFmpeg and x265 binaries.

How it gets in without touching either project: `phifmpeg build` adds
`-Wl,--undefined=phix_anchor` and `-lphix` to their link flags
(FFmpeg's `--extra-ldflags`/`--extra-libs`, x265's
`CMAKE_EXE_LINKER_FLAGS`). The undefined reference makes the linker take
the object that defines `phix_anchor` out of the archive; the same object
holds a function pointer in `.init_array`, which the C runtime calls before
`main`. The release profile's `codegen-units = 1` is what guarantees the
two are in one object (checked with `llvm-ar t` / `llvm-objdump -h`: the
`phix` member has both `.init_array` and `phix_anchor`).

`#![no_std]` with the `libc` crate only: no Rust runtime, no allocator,
nothing that could clash with musl, libc++ or libunwind in the final link,
and every path that may run in a signal handler is plain syscalls and
atomics. Panics abort.

The constructor does nothing unless a `PHIX_*` environment variable asks,
so a binary with phix linked behaves like one without.

Build: only through `phifmpeg build` (the card target spec and the
patched LLVM rustc must load come from the stack). The library must pass
the stack's `phi-isa-audit` with nothing illegal.
