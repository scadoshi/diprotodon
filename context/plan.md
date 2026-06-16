# diprotodon — Progress

This file tracks where the project actually is — milestone status, decisions made, gotchas surfaced, and what's next.

## Where I am

**M1 / M2 / M3 / M4 complete.** GET / SET / DEL / EXISTS / EXPIRE / EXPIREAT / TTL / PERSIST / PING (with optional message echo) all working end-to-end over real RESP. Persistence shipped via hexagonal ports: snapshot baseline (`wincode` + temp+rename) plus an AOF (RESP-encoded mutations) replayed on startup. `redis-cli -p 3000` is the verified client. Session is generic over `R: Read` / `W: Write` / `CS: CacheService`; `SessionReader` owns the frame-accumulation buf and handles drain/Incomplete/hard-err paths correctly. Bad commands return `-ERR ...\r\n` SimpleError and the session continues.

**Graceful shutdown complete.** `Arc<AtomicBool>` shutdown flag, `TcpListener::set_nonblocking(true)` + 50ms throttle on `WouldBlock`, stdin EOF / "quit" / "exit" trigger shutdown, all spawned threads (persistence + sweeper + per-client) are collected via `JoinHandle` and joined cleanly before `Server::run` returns. Persistence and sweeper threads cooperate with the flag via short-tick (100ms × 100) sleep loops so shutdown latency is bounded.

**Testing posture.** 219 unit tests passing. Frame parser, Command-from-Frame, Reply serializer, Crlf, and SessionReader all have unit tests. `Cache` has a comprehensive unit suite (every public method, lazy-expiry, past-TTL semantics, bulk remove-expired) plus per-variant coverage of `Cache::execute`. The new persistence layer is fully covered: `Snapshot` round-trip (incl. atomic temp+rename, missing-file error), `Aof` (append → exact RESP bytes, replay, torn-tail tolerance, malformed-frame error, clear), and `Persister` (append + snapshot-then-clear checkpoint, post-snapshot logging resumes). `Service` tests verify `execute` never logs and `execute_logged` appends iff mutating. `Session::execute` has per-variant wire-byte assertions and `get_command` has happy-path + bad-frame + EOF coverage. The `CommandOutcome → Reply` mapping is exhaustively tested. Every error path on every command has explicit coverage in `resp/command.rs`. Tests grouped under `// ---------- name ----------` section headers for navigability. Shared fakes (`RecordingRepo`, `SharedWriter`, `TempPath` RAII temp-file helper) live in `src/lib/test_support.rs` behind `#[cfg(test)]` — no `tempfile` crate.

**Strategic phase shift.** From here forward, work is allocated by *what's novel vs. what's rehearsed*, not by milestone order. The user has already shipped LSM-style persistence (WAL + memtable + SSTable + compaction + bloom filters) in `~/Projects/nighthawk`. That makes append-only-log mechanics rehearsed muscle — AI-assisted is fine. The unrehearsed pieces (pub/sub fan-out, MULTI/EXEC, async migration) get hand-written.

## Testing next up

Coverage is comprehensive through M4. Outstanding tests are deferred to the features that drive them:

- **End-to-end `repl`** — happy multi-command flow ending in clean disconnect, and bad-command-mid-stream proving the session continues after a SimpleError. Lower-priority since `Session::execute` + `get_command` are both individually tested.
- **Concurrent persister stress** — multi-threaded append + concurrent snapshot, verifying the checkpoint serialization holds. Owed once we have a real load story.
- **Subscribed-mode session tests** (M5) — once Pub/Sub lands, sessions in subscribed mode need their command-restriction logic tested.
- **EXEC atomicity tests** (M6) — once MULTI/EXEC lands, the lock-once-across-the-queue property needs a test where a second session is blocked mid-EXEC and observes a consistent before/after state.

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
- [x] Track per-key expiry timestamps — unified `Entry { value: Vec<u8>, absolute_ttl: Option<u64> }` (absolute UNIX seconds). Iterated through two shapes: first inline `(Value, Option<Instant>)`, then sidecar `HashMap<Vec<u8>, u64>`, finally collapsed into a single `Entry` struct because perf gains from sidecar-for-low-TTL-usage didn't matter at this scale and the unified shape is simpler. `SystemTime`-based absolute seconds on disk over `Instant` because `Instant` is process-local and unserializable by design.
- [x] Single mutex over the `HashMap<Vec<u8>, Entry>` — no deadlock-via-ordering risk, sweep is small.
- [x] Relative + absolute TTL APIs (`set_relative_ttl`, `set_absolute_ttl`, `get_relative_ttl`, `get_absolute_ttl`, `remove_ttl`). EXPIRE feeds into relative; EXPIREAT feeds into absolute directly. Semantics mirror real Redis.
- [x] Lazy expiration on read — `get`, `contains`, `get_absolute_ttl` all drop expired keys on access so clients never see them between sweeps.
- [x] Active background sweep — `Cache::remove_expired()` plus a dedicated sweeper thread in `server.rs`. Hold-the-lock pattern (not snapshot) — defended for current scale because sweep is microseconds; would switch to snapshot+re-check if profiling showed tail-latency hurt.
- [x] Past-timestamp absolute TTL → immediate removal, return `1` if key existed. Matches real Redis EXPIREAT semantics; clients don't need to wrap calls in clock checks.
- [x] Insert of an "already expired" Entry is accepted (no clock check at the boundary). The next read removes it lazily — consistent with the rest of the expiry model. Real Redis does the same.
- [x] TTL command returns `:-2` for missing, `:-1` for no-TTL, `:n` for seconds remaining.

### M4 — Persistence ✅ (snapshot baseline + AOF; hybrid recovery)
- [x] **Architecture: hexagonal ports.** `CacheRepository` (outbound port) + `CacheService` (inbound port) defined in `domain/ports.rs`; the domain `Service<CR>` orchestrates cache execution + AOF append; the outbound `Persister` implements `CacheRepository`, composing an `Aof` and a `Snapshot` (each a newtype over a shared `PersisterInner` writer+path). Adapter errors map into a domain-owned `RepositoryError` at the boundary.
- [x] **Snapshot** — `wincode` serialize/deserialize of the whole `HashMap<Vec<u8>, Entry>`. `load` on startup (empty file → empty map). `store` writes to a `.tmp` sibling then atomically `rename`s over the live file — crash-safe, never a half-written snapshot.
- [x] **AOF write path** — `Service::execute_logged` classifies the command (`MutatingCommand::from_command` → `None` for reads), executes against the cache, then appends the mutation to the log. The AOF entry *is* RESP: `From<MutatingCommand> for Frame` + `Frame::write_to`, byte-for-byte what a client sends. Relative `EXPIRE` is normalized to absolute `EXPIREAT` at encode time so replay is time-invariant. Append mode (`OpenOptions::append`) so restarts extend rather than clobber.
- [x] **AOF replay on startup** — load snapshot for the baseline, then replay the log on top via the *same* parse path (`Frame::parse_one` → `Command::try_from` → cache-only `execute`, no re-logging). Trailing `Incomplete` frame = torn tail from a crash mid-append → stop cleanly, keep what parsed. Mid-stream parse error or unknown command = fatal (our own log, so an uninterpretable entry means the rebuild can't be trusted). Runs before clients connect, so per-command locking is fine.
- [x] **Compaction (snapshot-then-truncate)** — taking a snapshot truncates the AOF, so the log only ever holds mutations *since* the last snapshot. Held under the cache lock across both the snapshot write and the AOF clear so no mutation can be wiped in between; order is snapshot-first (durable baseline), then clear (a crash between only re-applies already-snapshotted commands — harmless). Persist task loops on a 10s interval in `server.rs`.
- [x] Crash safety — atomic temp+rename on snapshot; torn-tail-tolerant replay on the AOF.
- [ ] **Full AOF rewrite without blocking writers** — the current compaction holds the cache lock for the snapshot duration (clone-under-lock semantics). The novel version — consistent snapshot *without* stalling writers (fork+COW vs. copy-on-write structures vs. a rewrite buffer for concurrent writes) — is still future work. Fine at current scale; revisit under load.
- [ ] **fsync durability tier** — appends currently buffer through `BufWriter` (durable on flush/drop), no per-write `fsync`. A configurable `everysec`/`always` policy is future work.

### M5 — Pub/Sub 🚧 (in progress, `pubsub` branch)

**WIP status (sync-first path chosen — building the per-session writer thread by hand before any async migration):**

*Done — the session transport plumbing is in and verified end-to-end:*
- Session split into `ReadHalf` (parse + execute + emit) and `WriteHalf` (drain channel → socket), connected by a per-session `mpsc<Vec<u8>>`. `Session::split()` consumes self and hands fields to the two halves; `repl()` spawns the writer thread and runs the reader on the current thread. Only the writer thread touches the socket's write half.
- `Reply::to_bytes()` added so the reader can encode replies to `Vec<u8>` and `send()` them down the channel instead of writing directly. (Dedup candidate later: `to_bytes` duplicates `write_to`'s per-variant logic — could route one through the other.)
- Both regressions from the split are fixed: the writer thread now `write_all(...).and_then(flush)`, and the reader loop swallows-and-retries `TimedOut | WouldBlock` (the 500ms read timeout from `server.rs`) instead of ending the session. Smoke-tested with `redis-cli`: PING/SET/GET/EXISTS/DEL all return correct replies and an idle interactive connection survives past the read timeout.

*Next, in rough order — this is where we're leaving off:*
1. **Subscriber logic + the actual commands.** `PUBLISH` / `SUBSCRIBE` / `UNSUBSCRIBE` don't exist yet — not parsed into `Command`, not executed. This is the bulk of the remaining work (see the unchecked bullets below).
2. **Shared registry.** `domain::channel` is only a stub today: `Channels = HashMap<Vec<u8>, Vec<Sender<Vec<u8>>>>` with a `ChannelsExt::subscribe`. It is **not** yet (a) shared across sessions behind a `Mutex`/`RwLock`, (b) keyed by session id in the inner collection (needed for O(1) `UNSUBSCRIBE` and disconnect cleanup — switch the inner `Vec<Sender>` to `HashMap<SessionId, Sender>`), or (c) reachable from `ReadHalf` (it needs an `Arc<Mutex<Channels>>` handle to register on SUBSCRIBE and fan out on PUBLISH).
3. **Wire `ReadHalf`'s dormant fields.** `id` and `subscriptions` (the per-session `HashSet<Vec<u8>>`) are currently dead — that's the two `dead_code` warnings. They light up when SUBSCRIBE/UNSUBSCRIBE land: `id` keys the registry's inner map; `subscriptions` lets no-arg UNSUBSCRIBE and disconnect-cleanup iterate this session's channels without scanning the whole registry.
4. **Fan-out path.** On PUBLISH, lock the registry, look up the channel, `send()` the serialized `["message", channel, payload]` into every subscriber's `sender` (the same `mpsc` their WriteHalf already drains), prune dead senders, return the surviving count. No new per-subscriber thread needed — the WriteHalf built above already *is* the subscriber output thread.

*Decisions still open:* slow-subscriber policy (unbounded for v1 is fine), whether to enforce subscribed-mode command restriction now or defer it (leaning defer — simpler), and the `Reply` array/`Push` variant for multi-element pub/sub replies.

**Also owed:** session tests are commented out (the old inline `tests` mod assumed the single-threaded `execute`/`writer` shape). Re-home them against `ReadHalf`/`WriteHalf` once the API settles.

- [ ] **PUBLISH / SUBSCRIBE / UNSUBSCRIBE / PSUBSCRIBE / PUNSUBSCRIBE** — at minimum the first three; pattern subs can come after.
- [ ] **Subscription registry** — `HashMap<channel, Vec<SubscriberHandle>>` (and a parallel pattern registry) behind a `Mutex` or `RwLock`. SUBSCRIBE registers a handle; UNSUBSCRIBE / disconnect removes it. Reverse index per session so cleanup on disconnect is O(subscribed channels), not O(all channels).
- [ ] **Fan-out transport** — per-subscriber `std::sync::mpsc::channel` (or `crossbeam`); PUBLISH iterates the channel's subscriber list and `try_send`s a serialized message into each. Slow-subscriber policy decided up front: drop with a count, or disconnect, or unbounded queue (real Redis disconnects past a buffer limit). Pick one, write down why.
- [ ] **Subscriber output thread** — each subscribed session needs a way to push messages out of band while it's also waiting on client reads. Sync world: a dedicated writer thread per subscribed session draining the mpsc into the socket. Async world: a `tokio::select!` between the socket read and the broadcast receive — much cleaner, which is why this lands easier post-async.
- [ ] **Subscribed-state command restriction** — once a session is in subscribed mode, only `SUBSCRIBE`/`UNSUBSCRIBE`/`PSUBSCRIBE`/`PUNSUBSCRIBE`/`PING`/`QUIT` are legal. Per-session state flag; reject others with `-ERR`.
- [ ] **Wire format** — push messages are RESP arrays: `["message", channel, payload]` for plain subs, `["pmessage", pattern, channel, payload]` for pattern subs. Subscribe replies are also arrays (`["subscribe", channel, count]`). Means `Reply` may need a `RespArray` variant or a new `Push` type.
- [ ] **Decision: sync first or async first.** Sync mpsc-per-subscriber works and teaches the slow-consumer problem viscerally. Tokio `broadcast::channel` makes the registry trivial but hides the lesson. Recommendation: do the async migration first (it's owed anyway), then build Pub/Sub on top.

### M6 — MULTI / EXEC ⬜ (then)
- [ ] **Session state machine** — sessions get a `Mode { Normal, Queueing }` toggle. `MULTI` flips to `Queueing`; subsequent commands return `+QUEUED\r\n` instead of executing; `EXEC` flushes; `DISCARD` aborts.
- [ ] **Per-session command queue** — `Vec<Command>` on the `Session`. Commands are parsed (so syntax errors still reject immediately with `-ERR` and abort the transaction per Redis semantics — set a `tx_dirty` flag), just not executed.
- [ ] **Atomic EXEC** — take the cache lock *once* across the whole queue; execute every command; collect every `CommandOutcome` into an array reply. While the lock is held, no other session interleaves — that's the atomicity guarantee. This is the load-bearing part: it's why EXEC isn't just "loop and call execute_logged."
- [ ] **AOF interaction** — every queued mutation must hit the log. Options: (a) buffer per-command AOF frames and write them all under the EXEC lock; (b) wrap the whole block in a `MULTI`/`EXEC` envelope frame on disk and have replay re-execute under the same atomic semantics. (b) preserves atomicity on replay, (a) is simpler. Decide before coding.
- [ ] **Error semantics** — a *parse* error during `Queueing` poisons the transaction: `EXEC` rejects with `-EXECABORT`. A *runtime* error during EXEC (e.g. `INCR` on a non-numeric value, once that exists) does *not* abort — that command's reply is the error, the rest still run. Matches real Redis; future-proofs the design once data types broaden.
- [ ] **WATCH/UNWATCH (optimistic locking)** — deferred to a follow-up. Real Redis uses WATCH to abort EXEC if a watched key changed between WATCH and EXEC. Interesting but needs a per-key version counter or change-notification hook; punt past the base MULTI/EXEC landing.
- [ ] **Reply shape** — EXEC reply is a RESP array of N replies (one per queued command). Wants the same `RespArray`/`Push` Reply variant that Pub/Sub needs — build it once.

### M7 — Stretch ⬜
- [ ] RDB-style snapshot format, Streams (XADD/XREAD), RESP3, INCR/DECR + Lists/Hashes/Sets, WATCH, KEYS/SCAN cursor pattern

## Cross-cutting work owed

- **Graceful shutdown** — ✅ done. `Arc<AtomicBool>` flag, nonblocking listener with 50ms throttle, stdin-EOF/quit/exit as the trigger (no `ctrlc` crate), all spawned threads join cleanly. Persistence and sweeper threads check the flag on a 100ms-tick budget so shutdown latency is bounded.
- **Async migration** — currently `std::thread` per connection. Tokio rewrite owed before M5 (broadcast fan-out wants async). Doing it sync first was a deliberate detour to feel the threading model before async hides it.
- **Connection lifecycle on errors** — ✅ read-timeout wired. Accepted streams get `TcpStream::set_read_timeout(500ms)`; the timeout bubbles up to `repl`, which checks the shutdown flag between waits (`TimedOut`/`WouldBlock` → continue). So an idle client no longer wedges the repl against shutdown. `get_command` still writes errors back and continues on malformed frames (session stays alive).
- **Service / Session error types** — `SessionError` (renamed from `ReplError`) wraps `io::Error` + `ServiceError`; `ServiceError` is the domain union of `CacheError` + `RepositoryError`. The split has held up through the AOF work — `execute_logged` failures lift cleanly through `?`.
- **Logging migration** — `tracing` / `tracing-subscriber` are in `Cargo.toml` as prep. Plan: replace the scattered `println!` / `eprintln!` calls in `server.rs` and `session.rs` with structured `tracing` events (`info!` for connection lifecycle, `warn!` for recoverable errors like bad commands, `error!` for session-fatal). Add a `tracing_subscriber::fmt()` init in `main.rs`. Worth doing before AOF so debugging the persist path has real structured logs to grep.

## Hand-coding vs AI-assist allocation

This is the strategic split going forward. The user has already shipped LSM persistence in nighthawk; remaining milestones get sorted by whether the *concept* is rehearsed or novel.

### Worth hand-writing (novel muscle)

- **TTL / EXPIRE / active sweep (M3)** — ✅ done. Unified `Entry { value, absolute_ttl }`, lazy expiry on every read path + active sweep (hold-the-lock — defensible at current scale; would switch to snapshot+re-check under load).
- **Graceful shutdown** — ✅ done. `Arc<AtomicBool>` + nonblocking listener + stdin-EOF/quit trigger + JoinHandle collection and join on exit.
- **AOF rewrite/compaction design** — the *consistent-snapshot-without-blocking-writers* problem is still novel. Sketch the algorithm by hand (fork+COW vs. clone-the-HashMap vs. copy-on-write structures, how to buffer concurrent writes during rewrite, atomic swap at the end) before writing code. The snapshot strategy is the interesting bit; file mechanics are mechanical.
- **Async/tokio migration** — paradigm shift, not a feature. Hand-write to feel the model and to have the lived sync→async rewrite experience.
- **Pub/Sub fan-out (M5)** — different concurrency shape than request/response. mpsc-per-subscriber vs. broadcast tradeoffs, slow-subscriber handling, subscription registry under contention. Easier after async lands (tokio broadcast > std::sync::mpsc fan-out).
- **MULTI/EXEC (M6)** — first per-session state machine the codebase has. Lock-once-across-the-queue is the atomicity story; the AOF-envelope decision is the interesting secondary call (preserve atomicity on replay vs. flatten and lose it). Decoupled from async; can land before or after.

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
3. ~~**AOF + snapshot by hand** (M4)~~ — done; hexagonal ports landed (`CacheRepository`, `CacheService`, `Service<CR>` orchestrator), snapshot via temp+rename, AOF replay is RESP through the same parse path.
4. **Async/tokio migration by hand** — paradigm shift, rewrite experience. Owed before M5; Pub/Sub fan-out is the forcing function.
5. **Pub/Sub by hand** (M5) — broadcast fan-out + slow-subscriber policy + per-session subscribed-mode state machine. Build the `RespArray`/`Push` reply variant here; reused by EXEC.
6. **MULTI/EXEC by hand** (M6) — first per-session state machine with cross-command atomicity. The lock-once-across-the-queue execution path is the load-bearing lesson; AOF envelope-vs-flat is the secondary decision.
7. AI sweeps in between or after: more commands (INCR/DECR/MGET/Lists/Hashes), tracing migration, README updates.

## Discipline note

Each milestone gets its own commit (or small series). Don't merge milestones. Easier to compare against the Go sibling later.
