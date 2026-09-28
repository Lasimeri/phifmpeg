# layout.rs

The build root and what lives under it:

| path | holds | written by |
| --- | --- | --- |
| `src/ffmpeg`, `src/x265` | the pristine checkouts | `fetch` |
| `src/nasm-<version>` | nasm's unpacked source | `fetch` |
| `downloads/` | tarballs, checked against `pins.toml` | `fetch` |
| `tools/bin/nasm` | the host assembler the `host` variant needs | `fetch` |
| `prefix/<variant>` | x265 installed for a variant (`libx265.a`, headers, `x265.pc`) | `build` |
| `build/<variant>` | out-of-tree FFmpeg builds (`c`, `host`); `ffmpeg_g` is the unstripped binary, `ffmpeg` the stripped one | `build` |
| `build/x265-<variant>` | out-of-tree x265 builds, with the `x265` CLI | `build` |
| `phix-build.log` | the last card workspace build | `build` (card variants) |
| `jobs/<job>/` | one transcode: `in/` (segments), `out/` (encoded segments, one file per device), `segments.txt` (the concat list), `segments.log` (the per-segment report) | `transcode` |

The card workspace itself builds into `card/target/` in the repository
(git-ignored), not under the build root.

The root is `PHIFMPEG_BUILD`, else `<repo>/build` (git-ignored; on the
development host it is a symlink to a second disk, because the build trees,
jobs and test media take several GB; test media are put there by hand, in
`media/`, and nothing reads that directory by name). A root with whitespace
is refused: FFmpeg's configure and make expand paths unquoted.
