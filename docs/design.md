# Design

## Goal

Real-time HEVC transcoding with the two Xeon Phi 3120 cards, used the way
a host uses NVDEC and NVENC: a stream goes in and comes back transcoded in
real time, the cards doing every part they can finish in time and the host
the rest (the owner: the cards are co-CPUs). Two rules from the owner shape
everything:

1. **FFmpeg is not modified**, and neither is x265, its HEVC encoder. No
   patches, no new codecs inside them. The upstream trees are built as they
   are, with build flags as the only input, and `phifmpeg` refuses to build
   a tree that differs from its pinned commit.
2. **Everything around them is Rust.** The host command (build, schedule,
   verify), the card runner, the card runtime library.

The owner also asked that the cards run **at least two threads per core**.
That is a hardware fact as much as a wish: a Knights Corner core cannot
issue from the same thread on two consecutive cycles, so one thread per
core reaches at most half of the core's issue rate (measured in the stack:
[`docs/results/2026-09-15-vpu.md`](https://github.com/Lasimeri/Intel-Phi-3120A/blob/main/docs/results/2026-09-15-vpu.md),
summarised in its `docs/hardware.md`). The transcode runs up to five
encoders per card, each with a 28-thread x265 pool, 2 frame threads and 2
decode threads, so about three threads per core, and the kernel spreads
them (pinning them made it slower; see the
[results](results/2026-09-27-cards-and-host-1080p60.md)).

## Where FFmpeg runs

On the cards. The alternative, FFmpeg on the host with its vector
instructions shipped to the card one region at a time (the way the sibling
Intel-Phi-AVX512's phi512 runs AVX-512 programs), cannot be real time: a
codec calls its vector functions hundreds of thousands of times per second
on blocks of a few hundred bytes, and every trip over PCIe costs
microseconds before any work is done. The sibling measured llama.cpp at
about 1000x too slow on that path for the same reason.

So FFmpeg is built for the card by the stack's compiler (knc64-x87 ABI:
x87 floating point, no SSE, no CMOV) and runs there as a normal static
program, with its threads spread over the 57 cores.

## The cards as co-processors

The owner's framing (2026-09-27): the cards are co-CPUs; they process
what they can and the host performs the rest. For a transcode that is a
scheduling problem, solved in `phifmpeg transcode`
([`host/phifmpeg/src/transcode.md`](../host/phifmpeg/src/transcode.md)):

- the unit of work is a segment between keyframes, independent of every
  other, so any device can take any segment;
- real time is a deadline per segment (its end in the video plus a fixed
  latency), and a card gets a segment only when its measured speed says it
  will make that deadline;
- the host encodes everything else, and backs up any card segment that
  runs late, so the cards add capacity without ever putting real time at
  risk.

Measured on 5 minutes of 1080p60: the cards encoded 19.3 percent, with no
missed deadline and no wasted card work
([results](results/2026-09-27-cards-and-host-1080p60.md)). Their share is
bounded by their C speed (card 0 about 9 fps) and by card 1's memory, of
which the AVX-512 worker holds 2.7 GB.

## The vector unit: not pursued

The cards run C; nothing in this repository uses the vector unit. The
owner set that stage aside on 2026-09-27. What was measured before it was,
kept for reference:

- x265's SIMD is worth 3.0x over its C on one host thread at SSE2, 4.8x at
  SSE4, 6.5x at AVX2 ([encode baseline](results/2026-09-27-encode-baseline.md)).
- On a card, about a dozen of x265's pixel functions take roughly 80
  percent of an encode: sums of absolute differences 28.0 percent, Hadamard
  costs 22.8, intra prediction 18.9, sub-pixel interpolation 8.4
  ([card profile](results/2026-09-27-card-profile.md)).
- The card's vector ISA is not SSE or AVX: Knights Corner removed MMX, SSE
  and AVX (Intel 327364-001 appendix B) and has 512-bit MVEX instead, with
  no 8-bit or 16-bit integer lanes (the same manual, appendix D.1.8).
- Loading 8-bit pixels from any address, widened to 32-bit lanes, takes
  the `vloadunpackld`/`vloadunpackhd` pair with the high half addressed
  64 bytes above the low half, for 8 and for 16 bytes alike: a probe on
  card 0 (since removed) got every one of 64 alignments right at +64, and
  +8 or +16 wrong as soon as the bytes crossed a 64-byte line. This agrees
  with the stack's `knc-mvex` (`conv.rs`, "64 above the low one's").

## Status

| stage | state |
| --- | --- |
| pinned, verified-pristine FFmpeg n9.0.2 | done |
| C-only build for the card, ISA audit clean | done |
| decode bit-exact on the card, thread sweep | done, [results](results/2026-09-27-c-baseline.md) |
| `asm` variant build (x86 SIMD in, for the vector unit) | built, then removed with that stage (2026-09-27) |
| encoder: x265 4.2, unmodified, C only on the cards | done, [results](results/2026-09-27-encode-baseline.md) |
| `phix` runtime: linked by flags, profiler | done |
| card runner, OOM guard, both cards | done |
| real-time transcode, cards + host | done, [results](results/2026-09-27-cards-and-host-1080p60.md) |
| card profile of an x265 encode | done, [results](results/2026-09-27-card-profile.md) |
| vector unit | not pursued (owner, 2026-09-27) |

## Rules for running on the cards

- Every phifmpeg process on a card runs with `oom_score_adj` 1000 (the
  runner sets it on itself, and its encoders inherit it), so that if
  memory runs out the kernel ends an encoder and never a resident service
  (the sibling's `phi-vpu-worker` holds up to 3 GB per card). Learned the
  hard way on 2026-09-27; see the [decode results](results/2026-09-27-c-baseline.md).
- Never hold a `phi run` session while a card works: the stack serves one
  session per card at a time, and every other `put`, `get` or command to
  that card waits behind it. Long work goes through the runner.
- A sibling's resident worker is not stopped without the owner's word;
  card slots are sized from the memory the card has available beside it.
