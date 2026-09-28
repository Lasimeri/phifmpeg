# 2026-09-27: x265 4.2 on card 0, and what 1080p60 needs

Host: Ryzen 7 5800X, kernel 7.2.6-1-cachyos. Card 0: Xeon Phi 3120A,
228 CPUs. x265 4.2 (`e444744c0397`) and FFmpeg n9.0.2, both unmodified,
built C only by `phifmpeg build --variant c` (x265 with
`ENABLE_ASSEMBLY=OFF`). Both binaries audit clean (x265 CLI: 585,971
instructions, 0 illegal).

## Content

Big Buck Bunny, the Blender Foundation's open movie (CC BY 3.0), 1080p
60 fps edition: `bbb_sunflower_1080p_60fps_normal.mp4.zip` from
download.blender.org, SHA-256 `68c45667...ec78`. Excerpt: 10 s starting
at 60 s, 600 frames, re-encoded on the host with libx265 `-preset medium
-crf 20`, keyint 120 (2,436,508 bytes, about 1.9 Mbit/s): the transcode
input, `bbb60.mkv`. Its first 120 frames as raw y4m (373 MB) are the
encode-only input. Real footage, not a test pattern: the encoder's work
depends on it.

## x265 silently loses its row parallelism above 64 CPUs

With default settings on the card, x265 printed `frame threads / pool
features: 5 / none` and encoded at **0.70 fps** (ultrafast). The cause is in
x265 4.2 (`source/common/threadpool.cpp`, `allocThreadPools`): a pool holds
at most 64 threads (`MAX_POOL_THREADS`, the width of its `uint64_t` sleep
bitmap). With 228 CPUs on one node and no libnuma it plans four pools, but
gives the first one all 228 threads, then walks past the populated node
entries looking for threads for the second; that pool's creation fails,
`numPools` is set to 0, and `encoder.cpp` then turns off WPP, pmode and pme
for want of a pool. Only the 5 frame threads work.

No source change needed: x265's own `--pools N` (or `-x265-params pools=N`
through FFmpeg) makes one pool of N threads. `--pools 57` restores
`wpp(34 rows)` and **3.70 fps**; 64 threads is no faster (3.71).

## Encode only (x265 CLI, 120 frames of `bbb60`, ultrafast)

| where | configuration | fps | CPU s per frame | notes |
| --- | --- | --- | --- | --- |
| card 0 | default pools (broken, see above) | 0.70 | | 5 threads busy |
| card 0 | `--pools 57`, one instance | 3.80 | 4.47 | 16 threads busy on average, peak RSS 670 MB |
| card 0 | 3 instances x `--pools 57` | 8.34 total | | 2.88 to 2.94 each |
| card 0 | 4 instances x `--pools 57` | 9.65 total | | 2.49 to 2.53 each, 2.7 GB |
| host, card-built C only | 16 threads | 47.3 | | WPP 34 rows |
| host, card-built C only | 1 thread | 8.06 | 0.125 | |
| host, system x265 (SIMD) | 16 threads | 162.0 | | |

Output agrees between the card and the host run of the same binary to
the reported precision (610.70 kb/s, average QP 37.89 both); a byte
comparison of the bitstreams is still to do.

A card thread costs about 36x a host thread here (4.47 against 0.125 CPU s
per frame), more than for decoding (28x).

## What SIMD is worth to x265 (host, one thread, ultrafast)

| `--asm` level | fps | vs C only |
| --- | --- | --- |
| C only (card-built binary) | 8.06 | 1.0x |
| sse2 | 24.1 | 3.0x |
| ssse3 | 24.6 | 3.0x |
| sse4 | 39.0 | 4.8x |
| avx | 39.3 | 4.9x |
| avx2 | 52.4 | 6.5x |

SSE2 alone, the simplest vector ISA (128 bits, no byte shuffle), is worth
3x. It is the first target for the vector unit; SSE4 the second.

## Full transcode (one FFmpeg process, HEVC in, libx265 out)

`ffmpeg -threads 57 -i bbb60.mkv -c:v libx265 -preset ultrafast
-x265-params pools=57`, 600 frames: **3.32 fps**, 180.7 s, 3,064 CPU s
(17 threads busy), peak RSS 1.04 GB. Decode is about 7 percent of the
work (0.33 of 5.1 CPU s per frame).

## Where 1080p60 stands

| step | fps | state |
| --- | --- | --- |
| one process, one card | 3.3 | measured |
| four processes, one card | about 9.7 | measured (encode only) |
| both cards | about 19 | projected; card 1 has 0.7 GB free while the AVX-512 worker holds its memory |
| x265's SSE2 functions on the vector unit | x3 on the host | next |

Two levers remain and both are needed: more encoder instances per card
(bounded by memory, 670 MB to 1 GB each) and the vector unit.

Later the same day: the budget above was measured end to end, with the
host taking what the cards cannot, in
[`2026-09-27-cards-and-host-1080p60.md`](2026-09-27-cards-and-host-1080p60.md)
(cards 19.3 percent of a 5-minute 1080p60 transcode in real time; card
slots settled at `pools=28`, 513 MB, five per card).
