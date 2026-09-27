# pins.toml

| pin | value | why this one |
| --- | --- | --- |
| FFmpeg | tag `n9.0.2`, commit `946fcce07b6d` | the newest release on 2026-09-27 (`git ls-remote --tags` of github.com/FFmpeg/FFmpeg; master was at `d62aef2e5043`), and the same version as the development host's own `ffmpeg`, so the host is an exact reference for every comparison |
| nasm | 3.02, SHA-256 `87336eba...` | the newest stable release on nasm.us; FFmpeg's x86 sources need NASM; no checksum is published upstream, so the first download's hash is the pin |

The tag object is `ce8f11b9fac4`; `commit` is what it peels to, which is
what `git rev-parse HEAD` reports after the clone and what `fetch`
compares.
