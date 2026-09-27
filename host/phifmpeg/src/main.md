# main.rs

The `phifmpeg` command: the host side of the port, everything that is not
FFmpeg. Subcommands so far:

| command | does |
| --- | --- |
| `phifmpeg stack` | where the stack, its toolchain, the ISA audit and the build root are |
| `phifmpeg fetch` | the pinned FFmpeg checkout (verified pristine) and the pinned nasm, built into the build root |
| `phifmpeg build --variant c\|asm` | FFmpeg for the card with configure flags only, then the stack's ISA audit |

The repository root is taken from `CARGO_MANIFEST_DIR`, so the binary is
meant to be run from this checkout (`cargo run -p phifmpeg -- ...` or
`target/debug/phifmpeg`). Errors print as `phifmpeg: ...` with their
context chain and exit 1.
