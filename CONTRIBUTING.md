# Contributing

These rules exist so that someone with the same cards and a fresh Arch
Linux install can reproduce every result here without asking anyone. The
sections are the same in every repository of the family (see
[The family](#the-family)); what differs is said where it applies.

## Languages

- **Rust** for everything written here: the host command (`host/`), and the
  card workspace (`card/`, built for the stack's `x86_64-knc-linux-musl`
  target): the `phix` runtime library (`no_std`) and the `phifmpeg-card`
  runner (`std`).
- **FFmpeg and x265 are never modified.** Not a patch, not a new file
  inside their trees. What they need is expressed as build flags (FFmpeg's
  configure, x265's CMake), link flags, or runtime options they already
  have (FFmpeg's `-threads`, x265's `pools`, `frame-threads`).
- **Shell** only for `scripts/check-docs.sh`, the family's shared
  repository-hygiene script.
- **Never Python or JavaScript** for anything here.

## Documentation

- Every code file (`.rs`, `.c`, `.h`, `.S`, `.sh`, `.json`, `.config`) has
  a sibling `.md` with the same stem in the same directory: purpose, the
  hardware or document facts the code depends on (with the source named),
  invariants, how to test it. A change to behaviour changes its `.md` in
  the same commit.
- Every public Rust item has a doc comment. Comments explain intent and
  hardware contract, not syntax.
- No em or en dash characters anywhere, commit messages included. Use
  commas, colons, parentheses, or `--`.
- Relative links between Markdown files must resolve. A file in a sibling
  repository is linked on GitHub, never named as if it were here.
- `scripts/check-docs.sh` enforces the sibling, dash and link rules
  (`make docs-check`, the first step of `make check`).

## Measurements

- Every hardware claim names its source: an Intel document number and
  section, a file and function in a named source tree, or a measurement
  made on this machine with the command shown.
- Results go under `docs/results/` with the date, the host kernel, and the
  exact command. Every speed number is verified, not only timed: a decode
  matches the host's frame hashes exactly; an encode decodes clean with
  every frame and matches the host's encode of the same input in PSNR;
  and the run says which card, how many slots and threads, and which
  device did which part.
- On a card, every phifmpeg process runs with `oom_score_adj` 1000, so a
  memory shortage ends an encoder and never a sibling's resident worker.
- `make check` (docs, format, lint, build, tests) needs no card and no
  stack: the card crates are formatted, linted and type-checked for the
  host there, the card runner's protocol is tested on the host with `sh`
  as the encoder, and the card build itself is only `phifmpeg build`. What
  needs a card is run by hand against one that is up (`phifmpeg
  transcode`, the card profiler) and recorded under `docs/results/`.

## The family

| repository | what | finds its dependency by |
| --- | --- | --- |
| [Intel-Phi-3120A](https://github.com/Lasimeri/Intel-Phi-3120A) | the cards' software stack: daemon, kernel, boot, storage, the `phi` CLI, the cross toolchain | (none) |
| [Intel-Phi-AVX512](https://github.com/Lasimeri/Intel-Phi-AVX512) | the cards as an AVX-512 co-processor: phi512, the card worker, `libggml_phi.so` | `PHI_STACK_ROOT`, `phi` on PATH, a checkout next to it, `$HOME` |
| [Intel-Phi-Jev](https://github.com/Lasimeri/Intel-Phi-Jev) | `xks`, a local Jev (System One) | `PHI_AVX512_ROOT`, a checkout next to it, `$HOME` |
| [Mechanical-Jev](https://github.com/Lasimeri/Mechanical-Jev) | `mjev`, the asking side of Jev | `MJEV_XKS`, `xks` on PATH, a checkout next to it, `$HOME` |
| phifmpeg (this one) | FFmpeg on the cards | `PHI_STACK_ROOT`, `phi` on PATH, a checkout next to this one, `$HOME` |

- A dependency is found in that order, as a checkout under its GitHub
  clone's name (`Intel-Phi-3120A`) or the spaced one (`Intel Phi 3120A`).
  Nothing of a sibling is copied here: the compiler, sysroot and ISA audit
  are the stack's, used through its `toolchain/env.sh` and `host/target`.
- The cards are shared. Intel-Phi-AVX512's `phi-vpu-worker` may be
  resident on either card; phifmpeg never stops it without the owner's
  word, and restores it with that repository's `scripts/phi-vpu.sh start`
  if it is lost.
- Everything downloaded here is pinned in `pins.toml` and checked on every
  fetch ([`docs/pins.md`](docs/pins.md)).

## Git

- One subject line that says what changed (a leading `Area:` is fine), then
  the why. `make check` before every commit, push after.
- Never commit FFmpeg sources, build trees, test media, or card binaries.
- MIT license ([`LICENSE-MIT`](LICENSE-MIT)) for this repository's own code.
