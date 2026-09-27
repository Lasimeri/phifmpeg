# phifmpeg

An unmodified FFmpeg on two Intel Xeon Phi 3120 coprocessor cards (Knights
Corner, 57 cores and 228 threads each), aiming at real-time HEVC decode and
encode the way a host uses NVDEC and NVENC.

- **FFmpeg is not modified.** The pinned upstream release (n9.0.2) is built
  as it is; configure flags are the only input, and the build refuses a
  tree that differs from the pinned commit.
- **Everything around it is Rust**: the host command, the card-side
  translator that runs FFmpeg's SIMD functions on each core's 512-bit vector
  unit, the launcher that spreads the work at two or more threads per core.

The cards run the mainline Linux port and the cross toolchain of
[Intel-Phi-3120A](https://github.com/Lasimeri/Intel-Phi-3120A); this
repository builds on that and copies nothing from it.

## State (2026-09-27)

| | |
| --- | --- |
| FFmpeg n9.0.2 for the card, C only | builds, ISA audit clean (4.49 M instructions, 0 illegal) |
| HEVC decode on card 0 | **bit-exact** (300/300 frame hashes match the host); 1080p30 at **29.8 fps** with 228 threads, 25.2 with 114 |
| x265 4.2 (HEVC encoder) for the card, C only | builds, audit clean; linked into FFmpeg as `libx265` |
| encode on card 0 (BBB 1080p60, ultrafast) | 3.8 fps per instance, **9.7 fps** with 4 instances; full transcode 3.3 fps per process |
| FFmpeg and x265 with their x86 SIMD, for the translator | builds; the card lacks those instructions, so they run only through the translator |
| translator (SIMD to the VPU) | next: x265's SSE2 functions (3x on the host) |
| both cards, host-facing transcode | not started |

Numbers and how they were checked: [decode](docs/results/2026-09-27-c-baseline.md), [encode and the 1080p60 budget](docs/results/2026-09-27-encode-baseline.md).
How the pieces fit and why: [`docs/design.md`](docs/design.md).

For scale, the same decode on the host: 596 fps with FFmpeg's own SIMD,
1042 fps on NVDEC. The cards are not going to match NVDEC; the goal is
real time, with the host's CPU left free.

## Build

Requirements: Intel-Phi-3120A with its toolchain built (its
`docs/reproducibility.md`, through the P2 toolchain stage, including LLVM
patch 0010) and a card up for anything that runs. Then, in this checkout:

```
cargo build
target/debug/phifmpeg stack                 # finds the stack and its toolchain
target/debug/phifmpeg fetch                 # pinned FFmpeg + nasm
target/debug/phifmpeg build --variant c     # C only, audited clean
target/debug/phifmpeg build --variant asm   # with FFmpeg's SIMD (for the translator)
```

Build trees go to `build/` (or `PHIFMPEG_BUILD`), several GB.

## The repositories

| repository | what | how it is found |
| --- | --- | --- |
| [Intel-Phi-3120A](https://github.com/Lasimeri/Intel-Phi-3120A) | the cards' software stack: boots them, the cross toolchain, the ISA audit, the `phi` command | `PHI_STACK_ROOT`, else `phi` on PATH, else a checkout next to this one, else in `$HOME` |
| [Intel-Phi-AVX512](https://github.com/Lasimeri/Intel-Phi-AVX512) | the cards as an AVX-512 co-processor; its `phi-vpu-worker` may be resident on the cards | not used by this repository; shares the cards |
| phifmpeg (this one) | FFmpeg on the cards | |

[`CONTRIBUTING.md`](CONTRIBUTING.md) has the rules the repositories share.
MIT ([`LICENSE-MIT`](LICENSE-MIT)) for this repository's own code; FFmpeg
itself is LGPL 2.1 or later and is fetched, never vendored.
