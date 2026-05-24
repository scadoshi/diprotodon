# diprotodon — Progress

Lifted from the master study plan at `~/Work/appetizers/ideas/redis-mini-study-plan.md`. This file tracks where the project actually is; the master plan owns the broader interview-prep trajectory.

## Where I am

**M1 complete end-to-end.** `redis-cli -p 3000 ping` returns `PONG`. `SET foo bar` / `GET foo` / `DEL foo` all verified over real RESP. Session is generic over `R: Read` / `W: Write`; `SessionReader` owns the frame-accumulation buf and handles drain/Incomplete/hard-err paths correctly. Bespoke text `Command::TryFrom<&str>` deleted. Bad commands return `-ERR ...\r\n` SimpleError and the session continues.

**Testing posture.** Frame parser, Command-from-Frame, Reply serializer, Crlf, and SessionReader all have unit tests. Cache derives `Default` now (enables in-memory construction for tests). Domain `Command` has constructor/getter tests.

**Next focus:** AOF persistence (M4). Decision made: snapshot is going away in favor of true append-only log + AOF rewrite for compaction (size-based trigger, background task writes minimal command sequence to reconstruct current state, atomic rename in). LSM/SSTable approach explicitly rejected — that's nighthawk's territory, not Redis-authentic.

## Testing next up

Before AOF, close these test gaps:
- **`Session::execute` per-variant** — Vec<u8> writer + real Cache, assert exact RESP bytes for each Command variant (GET hit/miss, SET, DEL hit/miss, PING).
- **End-to-end `repl`** — Cursor of scripted RESP request bytes as R, Vec<u8> as W, assert exact response bytes. Two tests: happy multi-command flow ending in clean disconnect, and bad-command-mid-stream proving the session continues after a SimpleError.
- **`Cache` unit tests** — `Cache::default()` unlocks these. get hit/miss, set new/existing, delete hit/miss. Pure, no disk.
- **`Cache::init` / `persist`** — needs `CACHE_PATH` to become a parameter (or `tempfile` crate). Separate refactor, can wait.

## Status by milestone

### M0 — TCP echo server ✅
- [x] `TcpListener` bind + accept loop (`inbound/server.rs`)
- [x] Thread-per-connection via `std::thread::spawn`
- [x] Per-connection `Session` struct owns reader/writer halves

### M1 — Protocol + dispatch ✅
- [x] **Byte-slice utilities** — `Crlf` trait in `src/lib/inbound/resp/crlf.rs` with `is_crlf` and `split_crlf`. Contract A: `split_crlf` returns `None` when no CRLF is found. That `None` is the load-bearing Incomplete signal for the parser layer above.
- [x] **RESP parser layer** — `Frame::parse_one(&[u8]) -> Result<(Frame, &[u8]), FrameError>` in `src/lib/inbound/resp/frame.rs`. Iterative array parsing (no stack-recursion risk for large MGETs). Error variants: `Incomplete`, `Malformed`, `UnknownSigil`, `InvalidLength`, `MissingTerminator`. Full unit test coverage.
- [x] **`Frame → Command` mapping** — `impl TryFrom<Frame> for Command` in `src/lib/inbound/resp/command.rs`. Peel array → ASCII-lowercase verb → match on `b"get" | b"set" | b"del" | b"ping"` → arity check. Full unit test coverage.
- [x] **Serializer (`Reply`)** — `src/lib/outbound/resp/reply.rs`. Five variants. `SimpleInner` newtype validates no-CR/LF; `ok()`/`pong()`/`sanitized(...)` constructors for trusted/untrusted payloads. `write_to(&mut impl Write)` streams bytes via `write_all`. Full unit test coverage.
- [x] **Session wiring** — `Session<R: Read, W: Write>` generic. `SessionReader<R>` owns the frame-accumulation buf and handles drain (success), preserve (Incomplete), and clear (hard err). `execute` returns a `Reply` per Command variant. SessionReader has unit tests for read count/EOF and parse_frame drain behavior.
- [x] **Bad-command resilience** — malformed frames and unknown commands return `-ERR ...\r\n` via `SimpleInner::sanitized` (strips CR/LF from arbitrary error message bytes) without killing the session. Disconnect (0-byte read) cleanly returns the session.
- [x] **Smoke verified** — `redis-cli -p 3000 ping/set/get/del` all working over real RESP.
- [ ] Minor: in `inbound/resp/command.rs`, the `b"ping"` arm doesn't reject trailing args (no `TooManyParts` check). Should mirror the GET/DEL pattern. Tiny fix.
- [ ] Pre-existing nit: in `parse_one`, length parse runs before sigil check, so `+OK\r\n` returns `InvalidLength` instead of `UnknownSigil`. Only matters if you ever support `+`/`-`/`:` inbound (you won't, per scope).

### M2 — GET / SET / DEL ✅ (EXISTS pending)
- [x] `Cache` API: `get`, `set`, `delete` (returns prior value where relevant)
- [x] `Arc<Mutex<HashMap>>` behind a real method boundary
- [x] Generic `impl AsRef<str>` / `impl Into<String>` ergonomics
- [ ] EXISTS

### M3 — EXPIRE / TTL / PEXPIRE ⬜
- [ ] Track per-key expiry timestamps (rework value type)
- [ ] Lazy expiration on read
- [ ] Active background sweep

### M4 — Persistence 🟡 (snapshot exists; AOF is the upgrade target)
- [x] Snapshot serialize via `wincode`, load on `Cache::init`
- [x] Truncate + create file on persist
- [x] Lock released before disk I/O (drop guard before file ops)
- [x] Persist task loops on a 10s interval (server.rs)
- [x] **Decision: move to AOF** — LSM (nighthawk's approach) is rejected here; Redis is in-memory-first, AOF matches the semantic.
- [ ] **AOF write path** — append every state-mutating command (SET, DEL, eventually EXPIRE) to a log file. `fsync` on each write (or buffered with configurable durability).
- [ ] **AOF replay on startup** — replay the log to rebuild the in-memory state.
- [ ] **AOF rewrite (compaction)** — size-based trigger. Background task takes a consistent snapshot of current state, writes a minimal command sequence to a new file, buffers concurrent writes during rewrite, appends them on completion, atomically renames over old AOF. The hard problem: getting a consistent snapshot without blocking writers — Redis uses `fork()` for COW; in Rust threading, simplest is to `clone()` the HashMap under the lock (expensive but simple). Worth thinking about up front.
- [ ] Crash safety (still applies to whichever model): atomic rename via tempfile.
- [ ] Pre-AOF cleanup: `Cache::persist` serializes the whole HashMap under the lock (mutex is dropped before disk I/O, but serialize itself is held). Big caches stall writers. Worth fixing or letting AOF supersede it.

### M5 — Pub/Sub ⬜
- [ ] PUBLISH / SUBSCRIBE
- [ ] Fan-out (probably `std::sync::mpsc` per subscriber, or migrate to tokio broadcast)

### M6 — Stretch ⬜
- [ ] RDB snapshots, MULTI/EXEC, Streams, RESP3

## Cross-cutting work owed

- **Graceful shutdown** — Ctrl-C kills mid-loop, last writes lost. Plan below.
- **Async migration** — currently `std::thread` per connection. Tokio rewrite owed before M5 (broadcast fan-out wants async). Master plan assumes tokio from the start; doing it sync first was a deliberate detour to feel the threading model.
- **Connection lifecycle on errors** — `get_command` writes errors back and `continue`s; on a broken stream this can hot-loop. Audit when wiring shutdown.
- **`Session` vs `Cache` error types** — `ReplError` wraps both; the split has held up post-RESP, but worth revisiting once AOF lands.
- **Logging migration** — `tracing` / `tracing-subscriber` are in `Cargo.toml` as prep. Plan: replace the scattered `println!` / `eprintln!` calls in `server.rs` and `session.rs` with structured `tracing` events (`info!` for connection lifecycle, `warn!` for recoverable errors like bad commands, `error!` for session-fatal). Add a `tracing_subscriber::fmt()` init in `main.rs`. Worth doing before AOF so debugging the persist path has real structured logs to grep.

## Graceful shutdown plan

The server has no exit path. Ctrl-C kills the process mid-loop and any cache mutations from the last persist tick are lost. Fix this before M3.

### Goals

- Catch SIGINT (Ctrl-C) and SIGTERM (`kill <pid>`)
- Stop accepting new connections
- Run one final `cache.persist()` before returning
- Return cleanly from `Server::run`

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
