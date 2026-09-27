# main.rs

The `phifmpeg` command: the host side of the port, everything that is not
FFmpeg. Subcommands so far:

| command | does |
| --- | --- |
| `phifmpeg stack` | where the stack, its toolchain, the ISA audit and the build root are |
| `phifmpeg fetch` | the pinned FFmpeg and x265 checkouts (verified pristine) and the pinned nasm, built into the build root |
| `phifmpeg build --variant c\|asm\|host` | x265 and FFmpeg with build flags only: `c` and `asm` for the card (with `card/`: phix and the runner, all audited), `host` natively with all SIMD for the host's share |
| `phifmpeg prof <binary> <samples>` | the card profiler's samples (`PHIX_PROF`) as a per-function table |
| `phifmpeg transcode IN OUT` | real-time HEVC transcode shared between the cards and the host (`transcode.md`) |

The repository root is taken from `CARGO_MANIFEST_DIR`, so the binary is
meant to be run from this checkout (`cargo run -p phifmpeg -- ...` or
`target/debug/phifmpeg`). Errors print as `phifmpeg: ...` with their
context chain and exit 1.
