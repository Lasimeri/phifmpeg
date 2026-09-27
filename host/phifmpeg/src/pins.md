# pins.rs

Reads `pins.toml` at the repository root into typed pins: FFmpeg and x265 as
git repositories, tags and the commits the tags must peel to; nasm as a URL and
SHA-256. See [`docs/pins.md`](../../../docs/pins.md) for why each pin is
what it is.
