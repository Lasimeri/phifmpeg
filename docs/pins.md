# pins.toml

| pin | value | why this one |
| --- | --- | --- |
| FFmpeg | tag `n9.0.2`, commit `946fcce07b6d` | the newest release on 2026-09-27 (`git ls-remote --tags` of github.com/FFmpeg/FFmpeg; master was at `d62aef2e5043`), and the same version as the development host's own `ffmpeg`, so the host is an exact reference for every comparison |
| nasm | 3.02, SHA-256 `87336eba...` | the newest stable release on nasm.us; FFmpeg's x86 sources need NASM; no checksum is published upstream, so the first download's hash is the pin |
| x265 | tag `4.2`, commit `e444744c0397` | the newest tag on 2026-09-27 (`git ls-remote` of bitbucket.org/multicoreware/x265_git, upstream's home; master was at `b81f650e21e8`); FFmpeg's `libx265` is its HEVC encoder, since FFmpeg has none of its own |

The tag objects are `ce8f11b9fac4` (FFmpeg) and `e7a37608685a` (x265); `commit` is what each peels to, which is
what `git rev-parse HEAD` reports after the clone and what `fetch`
compares.

Test content is not pinned in this file because nothing is built from it;
its source and SHA-256 are in the results record that uses it
([`results/2026-09-27-encode-baseline.md`](results/2026-09-27-encode-baseline.md)).
