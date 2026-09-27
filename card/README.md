# card

The Rust code that runs on the cards, built for the stack's
`x86_64-knc-linux-musl` target (its `toolchain/rust`, build-std) by
`phifmpeg build`. A separate Cargo workspace from `host/`, because the
target and `build-std` differ.

- [`phix`](phix/src/lib.md): a `no_std` static library linked into the
  unmodified FFmpeg and x265 with link flags only. Today: the sampling
  profiler ([`PHIX_PROF`](phix/src/prof.md)) that measures where a
  transcode spends the card's time. Next: running the two projects' SSE2
  functions on each core's vector unit (see
  [`../docs/design.md`](../docs/design.md)).
