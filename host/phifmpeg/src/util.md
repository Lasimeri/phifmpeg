# util.rs

The helpers every command shares, so that none of them lives inside one
command's module by accident:

| helper | does | used by |
| --- | --- | --- |
| `run` | runs a command with inherited stdio, fails on a non-zero exit | `fetch`, `ffbuild` |
| `capture` | runs a command and returns its stdout; the error carries its stderr | `fetch`, `transcode` |
| `sha256_file` | hex SHA-256 of a file, read in 1 MiB pieces | `fetch` (the nasm tarball), `transcode` (is the card's copy of a binary current) |
| `jobs` | parallel jobs for host builds: `PHIFMPEG_JOBS`, else the CPU count (4 if unknown) | `fetch`, `ffbuild` |

Tested with the host's `true`, `false` and `sh`, and against the published
SHA-256 of `abc`.
