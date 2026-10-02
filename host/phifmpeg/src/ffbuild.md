# ffbuild.rs

`phifmpeg build --variant c|host`: x265 and FFmpeg from the pristine
trees. Build-system flags are the only input; each list is written to
`phifmpeg.flags` in its build directory and a change forces a fresh
configure.

| variant | for | assembly | audit | used by |
| --- | --- | --- | --- | --- |
| `c` | the cards | none | enforced: zero illegal instructions or the build fails | `transcode` (card slots) |
| `host` | the host | all SIMD (nasm), and FFmpeg's inline assembly | none (the host executes everything) | `transcode` (host slots) |

## Order

1. Both trees checked pristine (`fetch::pristine`).
2. `c` only: the card workspace (`build_phix`): `cargo clean`, then
   `cargo build -Zjson-target-spec -Zbuild-std=core,alloc,std,panic_abort`
   for the stack's target spec, with `RUSTC_BOOTSTRAP=1`, the stack's LLVM
   library for rustc first on `LD_LIBRARY_PATH`, and `knc-cc` as the
   target's linker. Always clean, because cargo's fingerprints do not cover
   that library (the stack's ADR 0007). The output lands in
   `card/target/x86_64-knc-linux-musl/release` (`card_target`, where
   `transcode` also finds the runner). Both products are audited and must
   be clean: `libphix.a` and `phifmpeg-card`. Log: `<build>/phix-build.log`.
   Then any `ffmpeg_g`, `ffmpeg`, `ffprobe_g`, `ffprobe` or x265 CLI older
   than the new `libphix.a` is deleted: neither FFmpeg's Makefile nor
   x265's Ninja files know the binaries depend on it (it comes in through
   link flags), and a stale `ffmpeg_g` was once left in place this way.
3. x265 with CMake and Ninja, installed into `<build>/prefix/<variant>`.
4. FFmpeg configured against it (host `pkg-config` with `PKG_CONFIG_LIBDIR`
   set to that prefix only and `PKG_CONFIG_PATH` removed, so no host
   library can be found), built with make.
5. `c`: `ffmpeg_g` and the x265 CLI audited.

The card build uses the stack's toolchain environment
(`Stack::toolchain_env`); the host build the plain environment, because
the stack's exports `CC=knc-cc`, which CMake would pick up.

## FFmpeg flags, card build

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
| `--disable-asm --disable-x86asm --disable-inline-asm` | the card has none of FFmpeg's x86 SIMD instructions, and inline assembly sits inside C functions (the CABAC decoder's uses CMOV) |

## FFmpeg flags, host build

`--prefix`, `--pkg-config-flags=--static`, `--disable-autodetect`,
`--enable-static --disable-shared`, `--disable-doc`, `--enable-pthreads`,
`--enable-zlib`, `--enable-gpl --enable-libx265`, and the pinned nasm.
Native: the host's own compiler, all assembly on.

## x265 CMake flags

- Card: cross (`CMAKE_SYSTEM_NAME=Linux`, `CMAKE_SYSTEM_PROCESSOR=x86_64`,
  `knc-cc`/`knc-c++` and LLVM `ar`/`ranlib` by absolute path),
  `CMAKE_EXE_LINKER_FLAGS=-static -Wl,--undefined=phix_anchor -L<dir>
  -lphix` (the CLI carries the runtime too), `ENABLE_ASSEMBLY=OFF`.
- Host: native, `ENABLE_ASSEMBLY=ON` with the pinned nasm.
- Both: `CMAKE_BUILD_TYPE=Release`, `ENABLE_SHARED=OFF`, `ENABLE_CLI=ON`
  (the `x265` binary is kept for encode-only timing), `ENABLE_LIBNUMA=OFF`.

## Notes

- Needs the stack's LLVM patch 0010 (SSE-class returns past two x87
  registers go through memory): without it `libswscale/cms.c` and
  `libavfilter/vf_lut3d.c` fail with "SSE register return with SSE
  disabled". See [`docs/results/2026-09-27-c-baseline.md`](../../../docs/results/2026-09-27-c-baseline.md).
- Running x265 on a card: always pass a pool size of at most 64
  (`-x265-params pools=N`). Its default pool layout breaks above 64 CPUs
  and turns off its row parallelism; see
  [`docs/results/2026-09-27-encode-baseline.md`](../../../docs/results/2026-09-27-encode-baseline.md).
- An `asm` variant (the card build with both projects' x86 SIMD assembled
  in) existed for a vector-unit stage that is not pursued; it was removed
  on 2026-09-27. Its audit is in the decode results record.
