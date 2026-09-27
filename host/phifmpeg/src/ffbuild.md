# ffbuild.rs

`phifmpeg build --variant c|asm`: FFmpeg for the card from the pristine
tree. Configure flags are the only input; they are written to
`<dir>/phifmpeg.flags` and a change forces a fresh configure.

Flags common to both variants, and why:

| flags | why |
| --- | --- |
| `--enable-cross-compile --target-os=linux --arch=x86_64` | the card is x86-64 Linux, but configure must not treat the host as the target |
| `--cc=knc-cc --cxx=knc-c++`, LLVM `ar/ranlib/nm/strip` | the stack's compiler: knc64-x87 ABI, no SSE, no CMOV (stack `toolchain/clang/knc-cc.md`) |
| `--pkg-config=false --disable-autodetect` | no host library may be picked up |
| `--enable-static --disable-shared --extra-ldflags=-static` | the card userland is static musl, no dynamic loader |
| `--enable-zlib --enable-pthreads` | zlib is in the stack's sysroot; threads are the point |
| `--disable-inline-asm` | inline assembly sits inside C functions (the CABAC decoder's uses CMOV), where no function boundary exists for the translator |

Variant `c` adds `--disable-asm --disable-x86asm`. The result must pass
the stack's `phi-isa-audit` with zero illegal instructions, or the build
fails. Variant `asm` keeps FFmpeg's external assembly (SSE2 to AVX2,
assembled by the pinned nasm): those instructions do not exist on the
card and run only through the translator, so the audit is printed as a
report.

Needs the stack's LLVM patch 0010 (SSE-class returns past two x87
registers go through memory): without it `libswscale/cms.c` and
`libavfilter/vf_lut3d.c` fail with "SSE register return with SSE
disabled". See [`docs/results/2026-09-27-c-baseline.md`](../../../docs/results/2026-09-27-c-baseline.md).
