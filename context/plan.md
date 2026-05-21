# diprotodon — Progress

Lifted from the master study plan at `~/Work/appetizers/ideas/redis-mini-study-plan.md`. This file tracks where the project actually is; the master plan owns the broader interview-prep trajectory.

## Where I am

Custom line-based protocol over TCP, thread-per-connection, in-memory `HashMap<String, String>` behind `Arc<Mutex>`, basic snapshot persistence via `wincode`. No TTL, no async.

**RESP parser is complete and tested** — `Value::parse_one(&[u8]) -> Result<(Value, &[u8]), ValueError>` in `src/lib/resp/parser/value.rs`. Module restructured into `src/lib/resp/{parser,serializer}/`. Serializer dir scaffolded but empty. Next M1 work: `Value → Command` mapping, then the serializer, then wiring both into `Session` to replace the bespoke text parser.

## Status by milestone

### M0 — TCP echo server ✅
- [x] `TcpListener` bind + accept loop (`run.rs`)
- [x] Thread-per-connection via `std::thread::spawn`
- [x] Per-connection `Session` struct owns reader/writer halves

### M1 — Protocol + dispatch 🟡 (RESP parser done, dispatch + serializer next)
- [x] Line-based reader (`BufReader::read_line`)
- [x] `Command` enum with `TryFrom<&str>` parser
- [x] Dispatch in `Session::execute`
- [x] **Byte-slice utilities** — `Crlf` trait in `src/lib/resp/parser/crlf.rs` with `is_crlf` and `split_crlf`. Contract A: `split_crlf` returns `None` when no CRLF is found. That `None` is the load-bearing Incomplete signal for the parser layer above — do not collapse it into a `Some` with an empty rest slice (would lie to the caller).
- [x] **RESP parser layer** — `Value::parse_one(&[u8]) -> Result<(Value, &[u8]), ValueError>` in `src/lib/resp/parser/value.rs`. Dispatches on sigil, recurses via `parse_array` (iterative — not stack-recursive; `MGET key1..key1000` won't blow the stack), bottoms out at `parse_bulk_string`. Error variants: `Incomplete`, `Malformed`, `UnknownSigil`, `InvalidLength(ParseLengthError)`, `MissingTerminator`. Tests cover the byte-counting property (interior `\r\n` in a bulk payload), empty array, nested array, leftover-bytes preservation, and every error variant.
- [x] Drop `TryFrom<&[u8]> for Value` — leftover-bytes contract is structural to streaming and `TryFrom` can't carry it. (Note: `resp_notes.md` running log still mentions `TryFrom` as the entry point — divergence is intentional.)
- [ ] **`Value → Command` mapping (next)** — second layer the notes describe. Take a `Value::Array(Vec<Value::BulkString>)`, validate shape ("first element is the verb, rest are args"), ASCII-uppercase the verb, dispatch to `Command`. Rejects non-array top-level frames (per scope decision in `resp_notes.md`).
- [ ] **Serializer** — `src/lib/resp/serializer/` dir scaffolded but empty. Output types per `resp_notes.md` line 35: SimpleString, BulkString, NullBulk, Integer, SimpleError. Open design choice: extend `Value` with output-only variants vs. build a separate `Reply` enum. Not decided.
- [ ] Wire RESP parser + serializer into `Session`; remove bespoke `TryFrom<&str>` text parser.
- [ ] PING/PONG — trivial once dispatch maps `Command::Ping` to `Reply::SimpleString("PONG")`.
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
