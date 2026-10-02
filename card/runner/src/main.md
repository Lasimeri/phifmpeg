# main.rs (phifmpeg-card)

The card side of `phifmpeg transcode`: a small `std` program for the card
target that encodes the segments the host drops into a job directory, at
most `<slots>` at a time.

Why it exists: the stack's daemon serves one control session per card at
a time, so a host that kept a `phi run` open per encoder blocked its own
transfers for minutes (one 0.6 MB `put` waited 159 s). The runner holds no
session; the host visits it briefly.

Contract (also in the module comment): `in/<name>.mkv` appears by rename
when complete (the host uploads under a dot name first, and dot files are
ignored); the runner claims it by renaming into `run/`, runs the command
template with `{in}`/`{out}` replaced, and publishes `done/<name>.mkv`
followed by `done/<name>.ok` (encode seconds), or `done/<name>.fail`
(status and log tail), or `done/<name>.cancelled` after a
`cancel/<name>`. `stop` ends it once idle.

The marker is the segment's last write and appears whole: the `cancel/`
request is consumed and the `run/` files removed first, then the marker
is written under a dot name and renamed into place. Until 2026-10-02 it was
written first and in place, so a reader could find it empty (the host
would have parsed zero seconds) or beside leftover working files; the
host-side protocol test caught the second intermittently. Checked with
`strace` on the runner's file calls: the marker's rename now follows every
`unlink` of that segment.

The runner writes 1000 to its own `oom_score_adj` before starting
anything, so it and its encoders are the out-of-memory killer's first
choice, ahead of any resident service on the card.

Built by `phifmpeg build` with the rest of `card/` (std from source,
`knc-cc` as the linker) and audited clean by the stack's `phi-isa-audit`.
The protocol is tested on the host with `sh` as the encoder
([`../tests/protocol.md`](../tests/protocol.md), run by `make test`); on
card 0 it was tested standalone: one segment claimed, encoded in 37.8 s,
published, `stop` honoured.
