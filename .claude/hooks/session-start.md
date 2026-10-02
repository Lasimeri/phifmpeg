# session-start.sh

The SessionStart hook for Claude Code on the web, registered in
`../settings.json`. It runs only there (`CLAUDE_CODE_REMOTE=true`), before
the session starts, and warms what `make check` needs: the crates of both
workspaces fetched and built for the host, the card runner's protocol tests
compiled, `rustfmt` and `clippy` present. It needs neither the stack nor a
card, and running it twice is harmless (cargo does nothing the second time).
