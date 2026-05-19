# diprotodon — Progress

Lifted from the master study plan at `~/Work/appetizers/ideas/redis-mini-study-plan.md`. This file tracks where the project actually is; the master plan owns the broader interview-prep trajectory.

## Where I am

Custom line-based protocol over TCP, thread-per-connection, in-memory `HashMap<String, String>` behind `Arc<Mutex>`, basic snapshot persistence via `wincode`. No TTL, no async.

RESP migration started — byte-splitter (`Clrf::split_crlf`) landed in `src/lib/resp/utils.rs` with tests. Parser layer is the next session's work.

## Status by milestone

### M0 — TCP echo server ✅
- [x] `TcpListener` bind + accept loop (`run.rs`)
- [x] Thread-per-connection via `std::thread::spawn`
- [x] Per-connection `Session` struct owns reader/writer halves

### M1 — Protocol + dispatch 🟡 (RESP parser in progress)
- [x] Line-based reader (`BufReader::read_line`)
- [x] `Command` enum with `TryFrom<&str>` parser
- [x] Dispatch in `Session::execute`
- [x] **Byte-slice utilities** — `Clrf` trait in `src/lib/resp/utils.rs` with `is_crlf` and `split_crlf`. Contract A: `split_crlf` returns `None` when no CRLF terminator is found in the buffer. That `None` is the load-bearing "Incomplete" signal for the parser layer above — do not collapse it into a `Some` with an empty rest slice (would lie to the caller).
- [ ] **RESP parser layer (next up)** — function over `&[u8]` that dispatches on sigil and recurses for arrays. Sits on top of `split_crlf`. Notes worked out in `resp_notes.md`.
  - Tri-state return shape still owed: custom enum (`Parsed::Ok | Err | Incomplete`) vs `Result<Option<T>, E>` vs nom-style `Err(Incomplete)`. Leaning custom enum — recursion will do explicit matches anyway, and self-documenting variants help. Decide when building.
  - `RespValue` enum shape: `Array(Vec<u8>)` (flat) vs `Array(Vec<RespValue>)` (recursive). User leaning recursive — "fine and fun and cool." Recursive is what the spec describes; flat would force re-parsing.
  - Errors only exist at this layer, not at `split_crlf`. Don't push error variants down into the byte splitter.
  - Then map `RespValue -> Command` (the second layer the notes describe).
- [ ] PING/PONG (trivial once parser lands)
- [ ] Replace bespoke `TryFrom<&str>` command parser with RESP-driven dispatch
- [ ] Minor: `Clrf` trait name vs `crlf` method spelling — pick one casing

### M2 — GET / SET / DEL ✅ (EXISTS pending)
- [x] `Cache` API: `get`, `set`, `delete` (returns prior value where relevant)
- [x] `Arc<Mutex<HashMap>>` behind a real method boundary
- [x] Generic `impl AsRef<str>` / `impl Into<String>` ergonomics
- [ ] EXISTS

### M3 — EXPIRE / TTL / PEXPIRE ⬜
- [ ] Track per-key expiry timestamps (rework value type)
- [ ] Lazy expiration on read
- [ ] Active background sweep

### M4 — Persistence 🟡 (snapshot done, AOF pending)
- [x] Snapshot serialize via `wincode`, load on `Cache::init`
- [x] Truncate + create file on persist
- [x] Lock released before disk I/O
- [x] **Persist loop is broken** — current `spawn` runs *once* after 10s, never again. Wrap in `loop`.
- [ ] Crash safety: write to `cache.tmp` then `rename` over `cache`
- [ ] Decide: keep snapshot model or move to true AOF (append every write)

### M5 — Pub/Sub ⬜
- [ ] PUBLISH / SUBSCRIBE
- [ ] Fan-out (probably `std::sync::mpsc` per subscriber, or migrate to tokio broadcast)

### M6 — Stretch ⬜
- [ ] RDB snapshots, MULTI/EXEC, Streams, RESP3

## Cross-cutting work owed

- **Graceful shutdown** — Ctrl-C kills mid-loop, last writes lost. Plan below.
- **Async migration** — currently `std::thread` per connection. Tokio rewrite owed before M5 (broadcast fan-out wants async). Master plan assumes tokio from the start; doing it sync first was a deliberate detour to feel the threading model.
- **Connection lifecycle on errors** — `get_command` writes errors back and `continue`s; on a broken stream this can hot-loop. Audit when wiring shutdown.
- **`Session` vs `Cache` error types** — `ReplError` wraps both; review whether the split still makes sense once RESP lands.

## Graceful shutdown plan

The server has no exit path. Ctrl-C kills the process mid-loop and any cache mutations from the last persist tick are lost. Fix this before M3.

### Goals

- Catch SIGINT (Ctrl-C) and SIGTERM (`kill <pid>`)
- Stop accepting new connections
- Run one final `cache.persist()` before returning
- Return cleanly from `Runner::run`

### Things to investigate

- `ctrlc` crate vs writing the signal handler directly with `signal-hook` or `nix`
- How to break out of `listener.accept()` — it blocks. Options: non-blocking listener + poll, `set_nonblocking(true)` + sleep, or shutdown via a second socket trick
- Sharing a "should I keep running" flag across threads — `Arc<AtomicBool>` is the obvious one but think about whether it's the right shape
- Whether existing connection threads should be drained, killed, or left to die when the process exits
- Once async/tokio lands, this rewrites entirely — `tokio::signal::ctrl_c` + `CancellationToken`

### Don't

- Reach for AI before reading the `ctrlc` docs and one example
- Add async just to get `tokio::select!` — the sync version teaches more

## Discipline note

Each milestone gets its own commit (or small series). Don't merge milestones. Easier to compare against the Go sibling later.
