# card

The Rust code that runs on the cards, built for the stack's
`x86_64-knc-linux-musl` target (its `toolchain/rust`, build-std) by
`phifmpeg build --variant c`, and audited clean. A separate Cargo
workspace from `host/`, because the target and `build-std` differ. A plain
`cargo` here builds for the host: `make check` uses that to format, lint
and type-check these crates without the stack (phix's test target is left
out, since a `no_std` panic handler cannot link beside `std`).

- [`phix`](phix/src/lib.md): a `no_std` static library linked into the
  unmodified FFmpeg and x265 with link flags only. It holds the sampling
  profiler ([`PHIX_PROF`](phix/src/prof.md)) that measures where the card
  spends its time, and does nothing unless asked.
- [`phifmpeg-card`](runner/src/main.md): the card side of `phifmpeg
  transcode`. Started once per job and per card, it encodes the segments
  the host drops into a job directory on the card's disk, a fixed number
  at a time, so the host never holds the card's single control session
  while an encode runs. Its protocol is tested on the host
  ([`runner/tests/protocol.md`](runner/tests/protocol.md)).
