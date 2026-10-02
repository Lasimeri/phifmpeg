# transcode.rs

`phifmpeg transcode IN OUT`: one real-time HEVC transcode shared between
the cards and the host, treating the cards as co-processors: they take
every segment they can finish in time, the host does the rest.

## Model

1. **Segments.** The input's first video stream is cut at keyframes into
   segments of about `--segment-seconds` (FFmpeg's segment muxer, stream
   copy). Each is decoded and encoded independently, so the input needs
   closed GOPs: with open GOPs (x265's default), the leading pictures of
   each cut are dropped by the decoder and the frame count comes up short
   (the count at the end catches it).
2. **Real time.** A segment becomes available when its last frame would
   have arrived from a live source, and is due `--latency` seconds after
   its end time in the video, like a broadcast delay.
3. **Placement.** On arrival a segment goes to an **idle** card slot whose
   estimate, times `--card-safety`, plus `--card-overhead`, finishes before
   the deadline less `--margin`; otherwise to the host. Only idle slots:
   segments arrive every couple of seconds, so a slot that frees up is
   refilled almost at once, and queueing behind a busy slot turned the
   spread of card encode times (57 to 85 s for equal segments) into a
   cascade of late, cancelled work.
4. **Learning.** A card's speed estimate starts at `--card-fps` and moves
   halfway to each finished segment's measured speed. A segment stopped
   unfinished moves it 30 percent toward that bound (never onto it: the
   minimum let one slow segment lock a card out for good).
5. **Backups.** A monitor gives the host a copy of any card segment still
   unfinished at the last moment the host could make the deadline. The
   first result wins; a winning host copy cancels the card's. A card
   segment that fails (for example its encoder ended by the card's
   out-of-memory killer, which `oom_score_adj` 1000 points at it first),
   or whose result cannot be fetched or submitted, goes to the host at
   once. Every one of these paths claims the segment's single host copy
   (`claim_backup`), so two host slots never encode one segment into the
   same file at once. A card result that arrives after the host's copy won
   is not fetched; its time only bounds the card's speed estimate.
6. **Failure.** The host is the last resort: it tries a segment twice
   (`HOST_ATTEMPTS`), and if both fail the transcode stops, cancels what
   the cards are on and returns the error, instead of waiting for a segment
   nothing will finish. Options that would hang or panic are rejected
   before anything starts (`check_opts`): no host slot, a speed, latency or
   segment length that is not a positive number, a negative margin.
7. **Assembly.** Segments are Matroska (they carry timestamps; raw HEVC
   with B-frames has none to copy), joined with FFmpeg's concat demuxer
   by stream copy, with the input's audio mapped through. The output's
   frames are counted.

## Cards: the runner

The stack serves one control session per card at a time (`phi.md`), so
nothing here holds one while a segment encodes. Each card runs
`phifmpeg-card` (`card/runner`), started detached at the beginning of a
job with the card's slot count and encoder command, working from
`/data/phifmpeg/jobs/<job>/` on the card's disk. One host thread per card
(`card_agent`) visits it: submit (`put` under a dot name, then `mv` into
`in/`), cancel (`touch cancel/<seg>`), collect about once a second (list
`done/`, `get` each finished segment, `rm`). At the end it cancels
whatever the card is still on and touches `stop`.

## Slots

| device | per slot | how many |
| --- | --- | --- |
| card | 2 decode threads, x265 `pools=28:frame-threads=2` (513 MB peak) | (`MemAvailable` minus `--reserve-mb`) / `--slot-mb`, at most `--max-card-slots` (5) |
| card that fits none | one small slot, `pools=14` (414 MB peak) | 1 if `--small-slot-mb` fits |
| host | 4 decode threads, x265 `pools=--host-pool` | `--host-slots` |

Card: the build root's `build/c/ffmpeg` (C only) and the runner from
`card/target/x86_64-knc-linux-musl/release`, both installed under
`/data/phifmpeg/bin/` on the card when the SHA-256 there differs. Host: the
build root's `build/host/ffmpeg` (all SIMD). Both are FFmpeg n9.0.2 with
x265 4.2. The card's thread counts (2 decode threads, 2 x265 frame
threads) and the host's (4 decode threads) are constants at the top of the
module; the pool sizes are options.

## Tested without a card

The decisions are pure functions with unit tests (`cargo test`):
`slot_plan` (slots from a card's free memory, with the measured cases: 647
MiB gives the small slot, 3.6 GiB five), `place` (the fastest idle slot
that makes the deadline, else none), `learned` and `bounded` (the speed
estimate's two updates), `claim_backup` (one host copy per segment, from
any path), `check_opts`, `parse_done_line` (the runner's `done/` listing)
and `x265_params`. The runner's side of the protocol is tested on the host
in [`card/runner/tests/protocol.md`](../../../card/runner/tests/protocol.md).
The threads, the `phi` sessions and the encoders themselves are exercised
only by a real transcode.

## Report

Per segment (written to `segments.log` in the job directory): frames,
device, encode seconds, completion time and slack to the deadline.
Printed: deadline misses, host backups (the monitor's and those after a
failed card attempt, and how many won), card attempts
by outcome with the slot-seconds they cost, final speed estimates, and
each device's share of the frames.

Measured results: [`docs/results/2026-09-27-cards-and-host-1080p60.md`](../../../docs/results/2026-09-27-cards-and-host-1080p60.md).
