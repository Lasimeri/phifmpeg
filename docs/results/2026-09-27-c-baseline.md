# 2026-09-27: FFmpeg n9.0.2, C only, on card 0

Host: Ryzen 7 5800X (16 threads), kernel 7.2.6-1-cachyos. Card 0: Xeon
Phi 3120A at 0000:2f:00.0, card kernel 7.2.3 with the stack's patches, 228
CPUs online. FFmpeg n9.0.2 (`946fcce07b6d`), unmodified, built by
`phifmpeg build --variant c`.

## Toolchain gap found and fixed

The first build failed in two files with "SSE register return with SSE
disabled": `libavfilter/vf_lut3d.c` and `libswscale/cms.c`, both returning
a struct of three floats by value. The stack's knc64-x87 ABI returned
scalar floats in ST0 and ST1 (its LLVM patch 0002) but had nowhere to put a
third. Probed shapes on the old compiler: `float`, `double`,
`{float,float}`, `{double,double}`, `_Complex float`, `_Complex double` all
compiled; `{float,float,float}` did not.

Fix: stack LLVM patch 0010: without SSE, an SSE-class return that needs
more than two x87 registers is returned through memory. Shapes that fit in
two keep their convention, so nothing that compiled before changes ABI
(musl's `cexpf`, `csqrtf` included). Checked: all ten probe shapes compile
with zero SSE instructions; a two-file test (`{float r,g,b}` and
`{float a,b}` returned across translation units) prints the right values on
the host and on card 0; the test binary audits clean.

## ISA audit

`phi-isa-audit ffmpeg_g`: 4,485,904 instructions, **0 illegal, 0
suspect**.

## Correctness

Test stream: `testsrc2` (FFmpeg's synthetic pattern), 1920x1080, 30 fps,
10 s, encoded by the host's libx265 (`-preset medium`, keyint 60),
6,030,544 bytes. Reference: the host's system `ffmpeg` n9.0.2 with its
assembly, `-f framemd5`.

- Card-built binary run on the host: **300 of 300 frame hashes identical**.
- Same binary on card 0: **300 of 300 frame hashes identical**.

## Speed

`ffmpeg -nostats -benchmark -threads T -i t1080.mkv -f null -`, the whole
decode of 300 frames; one run each (the spread between runs was not
measured yet, so treat differences under about 10% as noise).

| where | threads | wall | fps | CPU s | CPU / wall | peak RSS |
| --- | --- | --- | --- | --- | --- | --- |
| card 0 | 16 | 20.10 s | 14.9 | 96.1 | 4.8 | 124 MB |
| card 0 | 32 | 15.91 s | 18.9 | 96.4 | 6.1 | 226 MB |
| card 0 | 57 | 15.09 s | 19.9 | 98.2 | 6.5 | 381 MB |
| card 0 | 114 | 11.91 s | 25.2 | 101.6 | 8.5 | 766 MB |
| card 0 | 228 | 10.08 s | 29.8 | 108.0 | 10.7 | 1,405 MB |
| host, card-built C only | 16 | 0.79 s | 380 | 3.54 | | 158 MB |
| host, system ffmpeg (SSE/AVX2) | 16 | 0.50 s | 596 | 2.15 | | |
| host, NVDEC (`hevc_cuvid`, RTX 3090 Ti) | | 0.29 s | 1042 | 0.22 | | |

(Host rows: two runs each, the second agreeing within 1%.)

What the numbers say:

1. **The card is not busy.** CPU time is about 100 s whatever the thread
   count, so the work per frame is fixed (about 0.33 CPU s on one card thread,
   about 28x a host thread) and only the parallelism changes. At 228 threads,
   about 11 are busy on average. FFmpeg's HEVC frame threading, on a stream
   with B-frames and 60-frame GOPs, does not expose more. This, not the
   cores' speed, is the first limit.
2. **Real time for 1080p30 decode is within reach on one card in C
   alone**, but only at 228 threads, and only for this synthetic content.
3. **SIMD is worth about 40% of the CPU time on the host** (2.15 against
   3.54 CPU s). That is a rough ceiling for what the VPU can take off the
   card's per-frame cost, unless native kernels beat what the host's SIMD
   gets.
4. The NVDEC frame hashes differ from the reference. Probably a different
   output pixel format (NV12 from `hevc_cuvid` against yuv420p), not a
   decoding difference; not checked yet.

## Incident

The first sweep pushed card 0 out of memory at 114 and 228 threads, and
the card's kernel killed the idle `phi-vpu-worker` of the sibling
Intel-Phi-AVX512 (pid 32725, 2.9 GB resident). It was restarted with that
repository's `scripts/phi-vpu.sh -c 0 start` (same arguments, `-v 114`,
768 huge pages) and answers its status poll; the uploads it held were stale
and are replaced by the next `libggml_phi.so` open anyway. Rule since then:
every phifmpeg process on a card runs with `oom_score_adj` 1000, and the
sweep above was re-run that way without harm.

## The `asm` variant, audited

`phifmpeg build --variant asm` (FFmpeg's x86 assembly in, nasm 3.02)
builds and links. `phi-isa-audit` on its `ffmpeg_g`: **237,185
instructions the card cannot execute**, all in FFmpeg's assembly
functions: SSE2 128,337; AVX 59,534; AVX2 21,489; SSE 8,085; MMX 7,057;
AVX-512 (F, BW, DQ, VL, VBMI, VBMI2, VNNI) 6,677; SSSE3 3,289; FMA 1,276;
SSE4.1 1,025; smaller counts of SSE3, FMA4, XOP, AES, PCLMULQDQ, BMI1,
BMI2, POPCNT; 79 CMOV; one XGETBV (FFmpeg's CPU detection, behind the
OSXSAVE bit the card never sets). Also 3,967 multi-byte NOPs (nasm's
alignment padding), which the stack treats as suspect on this card.

This is the static upper bound. `-cpuflags` decides which ISA level
FFmpeg dispatches to, so what the translator must cover is the set of
functions a real decode reaches at the chosen level, measured next.
