# Design

## Goal

Real-time HEVC transcoding on the two Xeon Phi 3120 cards, used the way a
host uses NVDEC and NVENC: the host hands over a stream, the cards decode
and encode it, the host gets the result. Two rules from the owner shape
everything:

1. **FFmpeg is not modified.** No patches, no new codecs inside it. The
   upstream tree is built as it is, with configure flags as the only input,
   and `phifmpeg` refuses to build a tree that differs from the pinned
   commit.
2. **Everything around FFmpeg is Rust.** The host command, the card-side
   translator, the launcher and the tests.

The owner also asked that the cards run **at least two threads per core**
and use **each core's vector unit (VPU)**. Both are hardware facts as much as
wishes: a Knights Corner core cannot issue from the same thread on two
consecutive cycles, so one thread per core reaches at most half of the
core's issue rate (Intel 328209 and the stack's measurements); and the VPU
is where almost all of the card's arithmetic throughput is.

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

## The vector unit (next): a translator, not a fork

FFmpeg's speed on x86 comes from its hand-written SIMD functions (SSE2 up to
AVX2, NASM sources under `libavcodec/x86`). The card has none of those
instructions: Knights Corner removed MMX, SSE and AVX entirely and put a
different 512-bit vector ISA (MVEX) in their place (stack
`docs/research/isa-deletions.md`, Intel 327364 appendix B). The plan:

1. Build FFmpeg **with** its assembly (variant `asm`), unmodified.
2. Turn those paths on with FFmpeg's own `-cpuflags` option (the card's
   CPUID reports no SSE, so FFmpeg would otherwise never call them).
3. Link a Rust library, `phix`, into the binary with linker flags. At
   startup it installs a handler for the invalid-opcode signal; the first
   time one of FFmpeg's SIMD functions runs, the handler finds the function
   in the program's own symbol table, produces an equivalent that runs on
   the VPU, and redirects the function to it, so each function traps once.
4. Two tiers of equivalent:
   - **native kernels** for the functions that matter most, written for the
     VPU's shape: 16 lanes of 32 bits, with 8- and 16-bit pixels widened and
     narrowed by the load and store instructions themselves;
   - **generic translation** of any other SIMD function, instruction by
     instruction, for correctness.

Why two tiers: the VPU has no 8-bit or 16-bit integer lanes at all (stack
`knc-vector-library` notes, Intel 327364 appendix D.1.8). A literal
translation of byte-shuffle-heavy SSSE3 or AVX2 pixel code needs many VPU
instructions per original one and may lose to plain C. A kernel written
for the VPU's shape does not have that problem. Which functions get native
kernels is decided by measurement, not guessed.

Correctness gates, all from FFmpeg itself, unmodified:

- FFmpeg's `checkasm` runs every SIMD function against its C reference on
  random inputs; under `phix` it checks the translated and native versions.
- HEVC decoding is exact integer arithmetic, so a decoded stream must match
  the host's frame hashes (`-f framemd5`) exactly, every frame.

## Status

| stage | state |
| --- | --- |
| pinned, verified-pristine FFmpeg n9.0.2 | done |
| C-only build for the card, ISA audit clean | done |
| decode bit-exact on the card, thread sweep | done, [results](results/2026-09-27-c-baseline.md) |
| `asm` variant build | done (audit in the results record) |
| encoder: x265 4.2, unmodified, C only on the cards | done, [results](results/2026-09-27-encode-baseline.md) |
| `phix` runtime: linked by flags, profiler | done |
| card runner, OOM guard, both cards | done |
| real-time transcode, cards + host | done, [results](results/2026-09-27-cards-and-host-1080p60.md) |
| vector unit for x265's hot functions | next |

## Rules for running on the cards

- Every phifmpeg process on a card runs with `oom_score_adj` 1000, so that
  if memory runs out the kernel ends FFmpeg and never a resident service
  (the sibling's `phi-vpu-worker` holds up to 3 GB per card). Learned the
  hard way on 2026-09-27; see the results record.
