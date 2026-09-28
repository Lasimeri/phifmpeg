# fetch.rs

`phifmpeg fetch`, and `pristine`, the check every `build` runs first.

- **FFmpeg and x265**: `git clone --depth 1 --branch <tag>` when the
  checkout is missing, then `pristine`: HEAD must equal the pinned commit
  and `git status --porcelain` must be empty. Every `build` runs the same
  check on both trees first, so the rule "FFmpeg and x265 are never
  modified" is enforced, not just stated. Builds are out of tree, so a
  build never dirties a checkout.
- **nasm**: FFmpeg's and x265's x86 sources (for the `host` variant) are
  NASM syntax and the host has no nasm package installed; building it from
  the release tarball into the build root needs no root access. Skipped
  when `tools/bin/nasm` already reports the pinned version. nasm.us
  publishes no checksum files, so the pin records the SHA-256 of the first
  download (trust on first use) and every later fetch must match it.

The nasm build's parallelism is `util::jobs` (`PHIFMPEG_JOBS`, else all
CPUs).
