# pins.rs

Reads `pins.toml` at the repository root into typed pins: FFmpeg as a git
repository, tag and the commit the tag must peel to; nasm as a URL and
SHA-256. See [`docs/pins.md`](../../../docs/pins.md) for why each pin is
what it is.
