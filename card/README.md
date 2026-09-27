# card

The Rust code that runs on the cards, built for the stack's
`x86_64-knc-linux-musl` target (its `toolchain/rust`, build-std). A
separate Cargo workspace from `host/`, because the target and `build-std`
differ.

Planned here, not written yet (see [`../docs/design.md`](../docs/design.md)):

- `phix`: a static library linked into the unmodified FFmpeg with linker
  flags. At startup it installs an invalid-opcode handler; the first time
  one of FFmpeg's SIMD functions runs, it finds the function in the
  program's symbol table and redirects it to an equivalent that runs on the
  core's vector unit (a native kernel where one exists, a translation
  otherwise). Also a sampling profiler (`PHIX_PROF`) to measure where a
  decode spends its time on the card.
- The launcher: pins FFmpeg's threads at two or more per core, sets
  `oom_score_adj` 1000, and passes `-cpuflags` so FFmpeg's SIMD paths are
  taken.
