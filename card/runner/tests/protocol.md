# protocol.rs

The runner's job-directory contract, tested on the host: the runner is a
plain `std` program, so `cargo test -p phifmpeg-card` in `card/` builds it
for the host and drives it with `sh` as the encoder (`make test` runs
this). Each test starts its own runner on a fresh directory under the
system temp directory and kills it at the end.

| scenario | asserts |
| --- | --- |
| dot file in `in/` | left alone; nothing claimed |
| segment renamed into `in/` | claimed into `run/`, output published as `done/<name>.mkv`, then `done/<name>.ok` with the seconds; `run/` cleaned |
| encoder exits 7 with text on stderr | `done/<name>.fail` holds `exit status: 7` and the log tail; no `.mkv`, no `.ok` |
| `cancel/<name>` for a waiting segment | `done/<name>.cancelled` is `0`; the segment leaves `in/` |
| `cancel/<name>` for a running segment | the encoder is killed; `.cancelled` holds the seconds it ran; `run/` and `cancel/` cleaned |
| `stop` while idle | exit 0 |
| `stop` with two segments running | exit 0 only after both are published |

What is not covered here: `oom_score_adj` (a `/proc` write the test
cannot observe without root) and the card itself; the standalone run on
card 0 recorded in [`../src/main.md`](../src/main.md) covers those.
