# phifmpeg

An unmodified FFmpeg (with an unmodified x265) on two Intel Xeon Phi 3120
coprocessor cards (Knights Corner, 57 cores and 228 threads each), used
the way a host uses NVDEC and NVENC: real-time HEVC transcoding where the
cards take every part they can finish in time and the host does the rest.

- **FFmpeg and x265 are not modified.** The pinned upstream releases
  (FFmpeg n9.0.2, x265 4.2) are built as they are; build flags are the only
  input, and the build refuses a tree that differs from the pinned commit.
- **Everything around them is Rust**: the host command that builds,
  schedules and verifies; the card runner; the card-side runtime library
  (a sampling profiler).
- **The cards are co-processors.** `phifmpeg transcode` cuts the video at
  keyframes, gives each segment to a card when the card can finish it
  before its real-time deadline, and to the host otherwise; the host also
  backs up any card segment that runs late, so real time never depends on
  the cards.

The cards run the mainline Linux port and the cross toolchain of
[Intel-Phi-3120A](https://github.com/Lasimeri/Intel-Phi-3120A); this
repository builds on that and copies nothing from it.

## State (2026-09-27)

| | |
| --- | --- |
| FFmpeg n9.0.2 with libx265 for the card, C only | builds, ISA audit clean (5.11 M instructions with libx265 and phix linked, 0 illegal; x265 CLI 0.61 M, 0 illegal) |
| HEVC decode on card 0 | **bit-exact** (300/300 frame hashes match the host); 1080p30 at **29.8 fps** with 228 threads, 25.2 with 114 |
| x265 4.2 (HEVC encoder) for the card, C only | builds, audit clean; linked into FFmpeg as `libx265` |
| **real-time 1080p60 transcode, cards + host** | **`phifmpeg transcode`: 5 min of BBB 1080p60, 0 deadline misses, the cards encoded 31.3% (card 0 16.7%, card 1 14.7%, five slots each), host the rest; output verified (decodes clean, card and host encodes of the same segment within 0.06 dB). With the AVX-512 worker resident on card 1: 19.3%** |
| one card, C only | about 9.3 fps of 1080p60 ultrafast (5 encoder slots, memory-bound) |
| where a card spends an encode | a dozen x265 pixel functions take about 80% of it |
| vector unit | not used (not pursued, 2026-09-27); the cards run C |

Numbers and how they were checked: [decode](docs/results/2026-09-27-c-baseline.md), [encode and the 1080p60 budget](docs/results/2026-09-27-encode-baseline.md), [cards and host together](docs/results/2026-09-27-cards-and-host-1080p60.md), [where a card spends an encode](docs/results/2026-09-27-card-profile.md).
How the pieces fit and why: [`docs/design.md`](docs/design.md).

For scale, the same decode on the host: 596 fps with FFmpeg's own SIMD,
1042 fps on NVDEC. The cards are not going to match NVDEC; the goal is
real time, with the host's CPU left free.

## Build

Requirements:

- Intel-Phi-3120A with its toolchain built (its `docs/reproducibility.md`
  through the toolchain stage, including LLVM patch 0010 and the `dylib`
  LLVM variant that rustc loads for the card target), its `phi-isa-audit`
  built (`make build` there), and a card up for anything that runs.
- On the host: Rust with `rust-src` (the card crates build `std` from
  source), a C and C++ compiler, `make`, `cmake`, `ninja`, `pkg-config`,
  `git`, `curl`, `tar`, `bash`.

Then, in this checkout (`make ffmpeg` does the fetch and both builds):

```
cargo build
target/debug/phifmpeg stack                 # finds the stack and its toolchain
target/debug/phifmpeg fetch                 # pinned FFmpeg, x265 and nasm
target/debug/phifmpeg build --variant c     # card: C only, audited clean, plus card/ (runtime, runner)
target/debug/phifmpeg build --variant host  # host: the same sources with all their SIMD
target/debug/phifmpeg transcode IN.mkv OUT.mkv   # real-time HEVC, cards + host
```

The input needs closed GOPs (every keyframe an IDR); `--latency`
(default 150 s) is the real-time budget and must exceed what a card needs
for one segment (about 70 s for 2 s of 1080p60 at ultrafast).

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
