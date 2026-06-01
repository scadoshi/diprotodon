# diprotodon — Progress

This file tracks where the project actually is — milestone status, decisions made, gotchas surfaced, and what's next.

## Where I am

**M1 / M2 / M3 complete.** GET / SET / DEL / EXISTS / EXPIRE / EXPIREAT / TTL / PERSIST / PING (with optional message echo) all working end-to-end over real RESP. `redis-cli -p 3000` is the verified client. Session is generic over `R: Read` / `W: Write`; `SessionReader` owns the frame-accumulation buf and handles drain/Incomplete/hard-err paths correctly. Bad commands return `-ERR ...\r\n` SimpleError and the session continues.

**Graceful shutdown complete.** `Arc<AtomicBool>` shutdown flag, `TcpListener::set_nonblocking(true)` + 50ms throttle on `WouldBlock`, stdin EOF / "quit" / "exit" trigger shutdown, all spawned threads (persistence + sweeper + per-client) are collected via `JoinHandle` and joined cleanly before `Server::run` returns. Persistence and sweeper threads cooperate with the flag via short-tick (100ms × 100) sleep loops so shutdown latency is bounded.

**Testing posture.** Frame parser, Command-from-Frame, Reply serializer, Crlf, and SessionReader all have unit tests. Every error path on every command has explicit coverage in `inbound/resp/command.rs` (TooManyParts, NotEnoughParts, UnexpectedFrame, Utf8/ParseInt for EXPIRE's ttl). Domain `Command` has constructor tests. The `get_key`/`get_value`/`get_ttl` getter methods on `Command` were dropped — pattern matching at the use site is the idiomatic accessor; getters returned statically-impossible `None`s for most variants.

**Strategic phase shift.** From here forward, work is allocated by *what's novel vs. what's rehearsed*, not by milestone order. The user has already shipped LSM-style persistence (WAL + memtable + SSTable + compaction + bloom filters) in `~/Projects/nighthawk`. That makes append-only-log mechanics rehearsed muscle — AI-assisted is fine. The unrehearsed pieces (pub/sub fan-out, async migration) get hand-written.

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
- [x] **PING `TooManyParts` check** — fixed as part of M3; PING also now accepts an optional message bulk string and echoes it back (matches real Redis).
- [ ] Pre-existing nit: in `parse_one`, length parse runs before sigil check, so `+OK\r\n` returns `InvalidLength` instead of `UnknownSigil`. Only matters if you ever support `+`/`-`/`:` inbound (you won't, per scope).

### M2 — GET / SET / DEL / EXISTS ✅
- [x] `Cache` API: `get`, `set`, `delete`, `exists`
- [x] `Arc<Mutex<HashMap>>` behind a real method boundary
- [x] Generic `impl AsRef<[u8]>` / `impl Into<Vec<u8>>` ergonomics
- [x] EXISTS — full pipeline (RESP arm + Command variant + Cache::exists via `contains_key` + Reply::Integer)
- More commands (INCR, DECR, MGET, APPEND, STRLEN, etc.) deliberately deferred — proven extensible, marginal learning per new arm

### M3 — EXPIRE / EXPIREAT / TTL / PERSIST ✅
- [x] Track per-key expiry timestamps — sidecar map (`expiries: HashMap<Vec<u8>, u64>` of absolute UNIX seconds), chosen over inline `(Value, Option<Instant>)` so keys without TTL pay zero overhead. `SystemTime`-based absolute seconds on disk over `Instant` because `Instant` is process-local and unserializable by design.
- [x] Single mutex over both `values` and `expiries` (one `Inner` struct) — no deadlock-via-ordering risk, sweep is small.
- [x] Relative + absolute TTL APIs (`set_relative_ttl`, `set_absolute_ttl`, `get_relative_ttl`, `get_absolute_ttl`, `remove_ttl`). EXPIRE feeds into relative; EXPIREAT feeds into absolute directly. Semantics mirror real Redis.
- [x] Lazy expiration on read — `get`, `contains`, `get_absolute_ttl` all drop expired keys on access so clients never see them between sweeps.
- [x] Active background sweep — `Cache::remove_expired()` plus a dedicated sweeper thread in `server.rs`. Hold-the-lock pattern (not snapshot) — defended for current scale because sweep is microseconds; would switch to snapshot+re-check if profiling showed tail-latency hurt.
- [x] Past-timestamp absolute TTL → immediate removal, return `1` if key existed. Matches real Redis EXPIREAT semantics; clients don't need to wrap calls in clock checks.
- [x] TTL command returns `:-2` for missing, `:-1` for no-TTL, `:n` for seconds remaining.

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

- **Graceful shutdown** — ✅ done. `Arc<AtomicBool>` flag, nonblocking listener with 50ms throttle, stdin-EOF/quit/exit as the trigger (no `ctrlc` crate), all spawned threads join cleanly. Persistence and sweeper threads check the flag on a 100ms-tick budget so shutdown latency is bounded.
- **Async migration** — currently `std::thread` per connection. Tokio rewrite owed before M5 (broadcast fan-out wants async). Doing it sync first was a deliberate detour to feel the threading model before async hides it.
- **Connection lifecycle on errors** — `get_command` writes errors back and `continue`s; on a broken stream this can hot-loop. Open. The current shutdown path covers process exit; per-connection wedge cases (idle/stuck clients blocking inside `read`) still want `TcpStream::set_read_timeout` so the repl loop wakes up periodically to check the shutdown flag.
- **`Session` vs `Cache` error types** — `ReplError` wraps both; the split has held up post-RESP, but worth revisiting once AOF lands.
- **Logging migration** — `tracing` / `tracing-subscriber` are in `Cargo.toml` as prep. Plan: replace the scattered `println!` / `eprintln!` calls in `server.rs` and `session.rs` with structured `tracing` events (`info!` for connection lifecycle, `warn!` for recoverable errors like bad commands, `error!` for session-fatal). Add a `tracing_subscriber::fmt()` init in `main.rs`. Worth doing before AOF so debugging the persist path has real structured logs to grep.

## Hand-coding vs AI-assist allocation

This is the strategic split going forward. The user has already shipped LSM persistence in nighthawk; remaining milestones get sorted by whether the *concept* is rehearsed or novel.

### Worth hand-writing (novel muscle)

- **TTL / EXPIRE / active sweep (M3)** — ✅ done. Sidecar expiry map, lazy + active expiry, hold-the-lock sweep (defensible at current scale; would switch to snapshot+re-check under load).
- **Graceful shutdown** — ✅ done. `Arc<AtomicBool>` + nonblocking listener + stdin-EOF/quit trigger + JoinHandle collection and join on exit.
- **AOF rewrite/compaction design** — the *consistent-snapshot-without-blocking-writers* problem is still novel. Sketch the algorithm by hand (fork+COW vs. clone-the-HashMap vs. copy-on-write structures, how to buffer concurrent writes during rewrite, atomic swap at the end) before writing code. The snapshot strategy is the interesting bit; file mechanics are mechanical.
- **Async/tokio migration** — paradigm shift, not a feature. Hand-write to feel the model and to have the lived sync→async rewrite experience.
- **Pub/Sub fan-out (M5)** — different concurrency shape than request/response. mpsc-per-subscriber vs. broadcast tradeoffs, slow-subscriber handling, subscription registry under contention. Easier after async lands (tokio broadcast > std::sync::mpsc fan-out).

### AI-jet (rehearsed in nighthawk or mechanical extension)

- **AOF base path** — append every state-mutating command, fsync, replay on startup. Same shape as nighthawk's WAL → memtable replay with `Command` swapped for `Entry`. Mechanical.
- **File atomicity** — tempfile + rename. Known.
- **Background task scaffolding** — periodic-loop spawn pattern is already in `server.rs`.
- **More commands** — INCR, DECR, MGET, APPEND, STRLEN. Proven extensible in <14 minutes per command.
- **Logging migration** — `tracing` macros replacing `println!`/`eprintln!`. Pure mechanical.
- **Test gap closure** — Cache unit tests, end-to-end repl tests with scripted RESP bytes. Rote.
- **README updates** — writing, not engineering.

### Strategic order

1. ~~**EXPIRE/TTL by hand** (M3)~~ — done
2. ~~**Graceful shutdown by hand**~~ — done
3. **AOF design by hand, AI scaffolds the code** (M4) — durability story, fork-vs-clone snapshot decision
4. **Async/tokio migration by hand** — paradigm shift, rewrite experience
5. **Pub/Sub by hand** (M5) — easier post-async
6. AI sweeps in between or after: more commands, tracing migration, test gap closure, README updates

## Discipline note

Each milestone gets its own commit (or small series). Don't merge milestones. Easier to compare against the Go sibling later.
