# settings.json

Claude Code's project settings. It registers one hook: `SessionStart` runs
[`hooks/session-start.sh`](hooks/session-start.md), which on Claude Code
on the web warms both cargo workspaces before the session starts, so that
`make check` needs no network.
