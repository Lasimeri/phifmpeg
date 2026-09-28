# card

The Rust code that runs on the cards, built for the stack's
`x86_64-knc-linux-musl` target (its `toolchain/rust`, build-std) by
`phifmpeg build --variant c` or `asm`, and audited clean. A separate
Cargo workspace from `host/`, because the target and `build-std` differ.

- [`phix`](phix/src/lib.md): a `no_std` static library linked into the
  unmodified FFmpeg and x265 with link flags only. Today: the sampling
  profiler ([`PHIX_PROF`](phix/src/prof.md)) that measures where the card
  spends its time. Next: faster versions of x265's hot pixel functions for
  each core's vector unit (see [`../docs/design.md`](../docs/design.md)).
- [`phifmpeg-card`](runner/src/main.md): the card side of `phifmpeg
  transcode`. Started once per job and per card, it encodes the segments
  the host drops into a job directory on the card's disk, a fixed number
  at a time, so the host never holds the card's single control session
  while an encode runs.
