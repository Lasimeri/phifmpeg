#!/bin/bash
# SessionStart hook for Claude Code on the web: warm both Rust workspaces so
# `make check` (fmt, clippy, build, tests) runs without waiting on the network.
# Nothing here needs the stack or a card. See session-start.md.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"
rustup component add rustfmt clippy >/dev/null 2>&1 || true

# Host workspace: dependencies, then the debug build the tests and
# `target/debug/phifmpeg` use.
cargo fetch
cargo build

# Card workspace, for the host target (what `make check` checks); the
# runner's protocol tests build it as a test dependency.
cargo fetch --manifest-path card/Cargo.toml
cargo build --manifest-path card/Cargo.toml
cargo test --manifest-path card/Cargo.toml -p phifmpeg-card --no-run
