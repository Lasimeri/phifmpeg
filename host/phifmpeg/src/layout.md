# layout.rs

The build root and what lives under it:

| path | holds |
| --- | --- |
| `src/ffmpeg` | the pristine FFmpeg checkout |
| `src/nasm-<version>` | nasm's unpacked source |
| `downloads/` | tarballs, checked against `pins.toml` |
| `tools/bin/nasm` | the host assembler FFmpeg's `asm` variant needs |
| `build/<variant>` | out-of-tree FFmpeg builds (`c`, `asm`) |

The root is `PHIFMPEG_BUILD`, else `<repo>/build` (git-ignored; on the
development host it is a symlink to a second disk, because an FFmpeg build
tree and test media take several GB). A root with whitespace is refused:
FFmpeg's configure and make expand paths unquoted.
