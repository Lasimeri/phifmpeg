# phi.rs

The stack's `phi` command as phifmpeg uses it: `<stack>/scripts/phi.sh -c N`
with the verbs the stack documents as its interface (`put`, `get`, `run`).
Nothing below that interface (the daemon socket, the ring) is touched.

Facts it relies on, measured on 2026-09-27:

- **One session per card at a time.** The stack's daemon serves one
  client's command or transfer at a time and queues the rest (its
  `phictl/src/serve.rs`: "One client at a time holds the card's session").
  Three concurrent 5 s `run`s take 15.1 s. A long `run` therefore blocks
  every `put` and `get` to that card for its whole length; the transcode
  never holds one (see `transcode.md`).
- `run` returns the card command's exit status (a card `exit 3` came back
  as 3) and keeps stdout and stderr apart.
- `put` keeps the local file's mode, and moved 373 MB in 34.4 s
  (10.8 MB/s): far too slow for raw video, ample for compressed segments (a
  2 s 1080p60 segment is about 0.5 MB, 0.09 s).

`sh_ok` retries a failing command twice, for idempotent control commands
only: once, a `chmod` through `run` returned 101 with no output and did
not reproduce. `mem_available` reads the card's `MemAvailable`.
