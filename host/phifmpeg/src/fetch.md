# fetch.rs

`phifmpeg fetch`, plus helpers the other commands share (`capture`,
`sha256_file`, `jobs`).

- **FFmpeg**: `git clone --depth 1 --branch <tag>`, then
  `ffmpeg_pristine`: HEAD must equal the pinned commit and
  `git status --porcelain` must be empty. Every `build` calls the same
  check first, so the rule "FFmpeg is never modified" is enforced, not
  just stated. Builds are out of tree, so a build never dirties the
  checkout.
- **nasm**: FFmpeg's x86 sources are NASM syntax and the host has no nasm
  package installed; building it from the release tarball into the build
  root needs no root access. nasm.us publishes no checksum files, so the
  pin records the SHA-256 of the first download (trust on first use) and
  every later fetch must match it.

`PHIFMPEG_JOBS` sets the parallelism of host builds (default: all CPUs).
