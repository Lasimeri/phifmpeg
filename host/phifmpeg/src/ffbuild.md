# ffbuild.rs

`phifmpeg build --variant c|asm`: x265 and FFmpeg for the card from the
pristine trees. Build-system flags are the only input; each list is
written to `phifmpeg.flags` in its build directory and a change forces a
fresh configure.

Order: both trees checked pristine, x265 built with CMake and Ninja and
installed into `<build>/prefix/<variant>`, then FFmpeg configured against
it (host `pkg-config` with `PKG_CONFIG_LIBDIR` set to that prefix only,
`PKG_CONFIG_PATH` removed, so no host library can be found), built, and
both binaries audited.

FFmpeg flags, and why:

| flags | why |
| --- | --- |
| `--enable-cross-compile --target-os=linux --arch=x86_64` | the card is x86-64 Linux, but configure must not treat the host as the target |
| `--cc=knc-cc --cxx=knc-c++`, LLVM `ar/ranlib/nm/strip` | the stack's compiler: knc64-x87 ABI, no SSE, no CMOV (stack `toolchain/clang/knc-cc.md`) |
| `--pkg-config=pkg-config --pkg-config-flags=--static --disable-autodetect` | libx265 is found through its `.pc` in our prefix only; nothing is autodetected |
| `--enable-static --disable-shared --extra-ldflags=-static` | the card userland is static musl, no dynamic loader |
| `--extra-libs=-lc++abi -lunwind` | x265 is C++; its `.pc` names `-lc++`, and a static libc++ needs these two as well |
| `--enable-gpl --enable-libx265` | the HEVC encoder; libx265 is GPL, so the binary is |
| `--enable-zlib --enable-pthreads` | zlib is in the stack's sysroot; threads are the point |
| `--disable-inline-asm` | inline assembly sits inside C functions (the CABAC decoder's uses CMOV), where no function boundary exists for the translator |

x265 CMake flags: cross (`CMAKE_SYSTEM_NAME=Linux`,
`CMAKE_SYSTEM_PROCESSOR=x86_64`, the same compilers by absolute path),
`ENABLE_SHARED=OFF`, `ENABLE_CLI=ON` (the `x265` binary is kept for
encode-only timing), static link, `ENABLE_LIBNUMA=OFF`.

Variant `c` adds `--disable-asm --disable-x86asm` and
`ENABLE_ASSEMBLY=OFF`: both binaries must pass the stack's
`phi-isa-audit` with zero illegal instructions, or the build fails.
Variant `asm` keeps both projects' SIMD (nasm from the build root): those
instructions do not exist on the card and run only through the
translator, so the audit is printed as a report.

Needs the stack's LLVM patch 0010 (SSE-class returns past two x87
registers go through memory): without it `libswscale/cms.c` and
`libavfilter/vf_lut3d.c` fail with "SSE register return with SSE
disabled". See [`docs/results/2026-09-27-c-baseline.md`](../../../docs/results/2026-09-27-c-baseline.md).

Both builds link the card runtime `card/phix` by reference
(`-Wl,--undefined=phix_anchor -lphix`); `build_phix` compiles it first,
from clean, for the stack's card target, and audits it (must be clean).

Running x265 on the card: pass `--pools 57` (or `-x265-params pools=57`
through FFmpeg). Its default pool layout breaks above 64 CPUs and turns
off its row parallelism; see
[`docs/results/2026-09-27-encode-baseline.md`](../../../docs/results/2026-09-27-encode-baseline.md).
