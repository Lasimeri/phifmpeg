# ffbuild.rs

`phifmpeg build --variant c|asm|host`: x265 and FFmpeg from the pristine
trees. Build-system flags are the only input; each list is written to
`phifmpeg.flags` in its build directory and a change forces a fresh
configure.

| variant | for | assembly | audit | used by |
| --- | --- | --- | --- | --- |
| `c` | the cards | none | enforced: zero illegal instructions or the build fails | `transcode` (card slots) |
| `asm` | the cards | both projects' x86 SIMD (SSE2 to AVX2), assembled by the pinned nasm | printed as a report (the hits are those SIMD functions) | the vector-unit stage (not built); does not run on a card as it is |
| `host` | the host | all SIMD, and FFmpeg's inline assembly | none (the host executes everything) | `transcode` (host slots) |

## Order

1. Both trees checked pristine (`fetch::pristine`).
2. Card variants only: the card workspace (`build_phix`): `cargo clean`,
   then `cargo build -Zjson-target-spec -Zbuild-std=core,alloc,std,panic_abort`
   for the stack's target spec, with `RUSTC_BOOTSTRAP=1`, the stack's LLVM
   library for rustc first on `LD_LIBRARY_PATH`, and `knc-cc` as the
   target's linker. Always clean, because cargo's fingerprints do not cover
   that library (the stack's ADR 0007). Both products are audited and must
   be clean: `libphix.a` and `phifmpeg-card`. Log: `<build>/phix-build.log`.
3. x265 with CMake and Ninja, installed into `<build>/prefix/<variant>`.
4. FFmpeg configured against it (host `pkg-config` with `PKG_CONFIG_LIBDIR`
   set to that prefix only and `PKG_CONFIG_PATH` removed, so no host
   library can be found), built with make.
5. Card variants: `ffmpeg_g` and the x265 CLI audited.

Card variants build with the stack's toolchain environment
(`Stack::toolchain_env`); the host variant with the plain environment,
because the stack's exports `CC=knc-cc`, which CMake would pick up.

## FFmpeg flags, card variants

| flags | why |
| --- | --- |
| `--enable-cross-compile --target-os=linux --arch=x86_64` | the card is x86-64 Linux, but configure must not treat the host as the target |
| `--cc=knc-cc --cxx=knc-c++`, LLVM `ar/ranlib/nm/strip` | the stack's compiler: knc64-x87 ABI, no SSE, no CMOV (stack `toolchain/clang/knc-cc.md`) |
| `--pkg-config=pkg-config --pkg-config-flags=--static --disable-autodetect` | libx265 is found through its `.pc` in our prefix only; nothing is autodetected |
| `--enable-static --disable-shared` | the card userland is static musl, no dynamic loader |
| `--extra-ldflags=-static -Wl,--undefined=phix_anchor -L<card target dir>` | static, and the reference that makes the linker take the `phix` object (and with it its constructor) out of `libphix.a` |
| `--extra-libs=-lphix -lc++abi -lunwind` | the runtime library; x265 is C++ and its `.pc` names `-lc++`, and a static libc++ needs these two as well |
| `--enable-gpl --enable-libx265` | the HEVC encoder; libx265 is GPL, so the binary is |
| `--enable-zlib --enable-pthreads --disable-doc` | zlib is in the stack's sysroot; threads are the point; no documentation build |
| `--disable-inline-asm` | inline assembly sits inside C functions (the CABAC decoder's uses CMOV) |
| `c`: `--disable-asm --disable-x86asm`; `asm`: `--x86asmexe=<build>/tools/bin/nasm` | the variant's choice |

## FFmpeg flags, host variant

`--prefix`, `--pkg-config-flags=--static`, `--disable-autodetect`,
`--enable-static --disable-shared`, `--disable-doc`, `--enable-pthreads`,
`--enable-zlib`, `--enable-gpl --enable-libx265`, and the pinned nasm.
Native: the host's own compiler, all assembly on.

## x265 CMake flags

- Card variants: cross (`CMAKE_SYSTEM_NAME=Linux`,
  `CMAKE_SYSTEM_PROCESSOR=x86_64`, `knc-cc`/`knc-c++` and LLVM `ar`/
  `ranlib` by absolute path), `CMAKE_EXE_LINKER_FLAGS=-static
  -Wl,--undefined=phix_anchor -L<dir> -lphix` (the CLI carries the runtime
  too).
- Host variant: native, no linker flags.
- All: `CMAKE_BUILD_TYPE=Release`, `ENABLE_SHARED=OFF`, `ENABLE_CLI=ON`
  (the `x265` binary is kept for encode-only timing), `ENABLE_LIBNUMA=OFF`,
  and `ENABLE_ASSEMBLY=OFF` for `c`, `ON` with the pinned nasm for `asm`
  and `host`.

## Notes

- Needs the stack's LLVM patch 0010 (SSE-class returns past two x87
  registers go through memory): without it `libswscale/cms.c` and
  `libavfilter/vf_lut3d.c` fail with "SSE register return with SSE
  disabled". See [`docs/results/2026-09-27-c-baseline.md`](../../../docs/results/2026-09-27-c-baseline.md).
- Running x265 on a card: always pass a pool size of at most 64
  (`-x265-params pools=N`). Its default pool layout breaks above 64 CPUs
  and turns off its row parallelism; see
  [`docs/results/2026-09-27-encode-baseline.md`](../../../docs/results/2026-09-27-encode-baseline.md).
