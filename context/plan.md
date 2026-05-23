# diprotodon — Progress

Lifted from the master study plan at `~/Work/appetizers/ideas/redis-mini-study-plan.md`. This file tracks where the project actually is; the master plan owns the broader interview-prep trajectory.

## Where I am

Custom line-based protocol over TCP, thread-per-connection, in-memory `HashMap<String, String>` behind `Arc<Mutex>`, basic snapshot persistence via `wincode`. No TTL, no async.

**RESP layers are built and tested in isolation; Session wiring is the last unfinished step.** Inbound: `Frame::parse_one(&[u8]) -> Result<(Frame, &[u8]), FrameError>` in `src/lib/inbound/resp/frame.rs` and `TryFrom<Frame> for Command` in `src/lib/inbound/resp/command.rs`. Outbound: `Reply::write_to(&mut impl Write)` in `src/lib/outbound/resp/reply.rs` with five variants (SimpleString, SimpleError, BulkString, NullBulk, Integer) and a `SimpleInner` newtype that validates "no CR/LF" at construction. Domain: `Command` and `Cache` are both `Vec<u8>`-keyed/-valued for binary safety. `Command::Ping` exists. Hex layout: `src/lib/{domain,inbound,outbound}/`.

**What doesn't run yet:** `redis-cli ping` against the server won't get a PONG. `Session` still uses the bespoke text protocol (`BufReader::read_line` + `Command::TryFrom<&str>` + `format!`-based responses). The RESP layers are all dead code from `Session`'s point of view — visible as `dead_code` warnings on `Reply::write_to` and `SimpleInner::as_bytes`. **The next session's work is wiring `Session` to read RESP frames, dispatch through `Command`, and write `Reply` bytes back.**

## Status by milestone

### M0 — TCP echo server ✅
- [x] `TcpListener` bind + accept loop (`inbound/server.rs`)
- [x] Thread-per-connection via `std::thread::spawn`
- [x] Per-connection `Session` struct owns reader/writer halves

### M1 — Protocol + dispatch 🟡 (RESP layers done in isolation; Session wiring is the last step)
- [x] Line-based reader (`BufReader::read_line`) — to be replaced by RESP byte-buffer read loop
- [x] `Command` enum with `TryFrom<&str>` text parser — to be deleted once RESP wiring lands
- [x] Dispatch in `Session::execute` — still on text format, returning `format!`ed strings
- [x] **Byte-slice utilities** — `Crlf` trait in `src/lib/inbound/resp/crlf.rs` with `is_crlf` and `split_crlf`. Contract A: `split_crlf` returns `None` when no CRLF is found. That `None` is the load-bearing Incomplete signal for the parser layer above — do not collapse it into a `Some` with an empty rest slice (would lie to the caller).
- [x] **RESP parser layer** — `Frame::parse_one(&[u8]) -> Result<(Frame, &[u8]), FrameError>` in `src/lib/inbound/resp/frame.rs`. Dispatches on sigil, recurses via `parse_array` (iterative — not stack-recursive; `MGET key1..key1000` won't blow the stack), bottoms out at `parse_bulk_string`. Error variants: `Incomplete`, `Malformed`, `UnknownSigil`, `InvalidLength(ParseLengthError)`, `MissingTerminator`. Tests cover the byte-counting property (interior `\r\n` in a bulk payload), empty array, nested array, leftover-bytes preservation, and every error variant. (Type was renamed from `Value` to `Frame`, file from `value.rs` to `frame.rs`, mid-development — matches mini-redis vocabulary, pairs with `Reply` on the outbound side.)
- [x] Drop `TryFrom<&[u8]> for Value` (now `Frame`) — leftover-bytes contract is structural to streaming and `TryFrom` can't carry it.
- [x] **`Frame → Command` mapping** — `impl TryFrom<Frame> for Command` in `src/lib/inbound/resp/command.rs`. Pattern: peel array → first element is verb (BulkString) → ASCII-lowercase once → match on `b"get" | b"set" | b"del" | b"ping"` → per-verb arity check and arg unpacking. Error type `CommandFromFrameError` wraps `CommandError` via `#[from]`. Test coverage matches the parser-side coverage: ok paths (incl. case insensitivity), all error variants. Rejects non-array top-level frames as `UnexpectedValue`.
- [x] **Serializer (`Reply`)** — `src/lib/outbound/resp/reply.rs`. Five variants: `SimpleString(SimpleInner)`, `SimpleError(SimpleInner)`, `BulkString(Vec<u8>)`, `NullBulk`, `Integer(i64)`. `SimpleInner` is a private-field newtype validated via `TryFrom<&[u8]>` — guarantees no CR/LF in simple-string/simple-error payloads at construction time. `Reply::write_to(&mut impl Write)` streams the wire bytes piece-by-piece via `write_all` (no intermediate Vec). Test coverage roundtrips each variant against the spec-correct wire bytes, including the interior-CRLF binary-safety case for bulk strings, empty bulk vs null bulk distinction, and negative integers.
- [x] **`Command::Ping` added at all parser layers** — text parser, RESP byte-dispatch, and the execute match. PING is currently handled in `Session::execute` with a text `"pong\n"` write (because the rest of the pipeline is still text). Once RESP wiring lands, it becomes `Reply::SimpleString(SimpleInner::try_from(b"PONG")?)`.
- [x] **`Session::writer` wrapped in `BufWriter<TcpStream>`** — accumulates small writes (sigil, payload, terminator) into one syscall per flush. Caller-side decision; `Reply::write_to` stays agnostic to whether its writer is buffered.
- [ ] **WIRE IT UP (next session's work)** — `Session` needs to switch from text protocol to RESP. Concrete steps:
  - Add a `buf: Vec<u8>` field to `Session` for frame accumulation (persists across reads — pipelining + Incomplete recovery).
  - Replace `get_message`/`get_command` with a `read_frame(&mut self) -> Result<Option<Frame>, ...>` loop: try `Frame::parse_one(&self.buf)`; on `Ok((frame, rest))` capture `rest.len()` (NOT the slice — borrow checker), drain `self.buf` to keep only the leftover, return frame; on `Err(Incomplete)` read more bytes into `self.buf` (raw `read` into a stack chunk + `extend_from_slice`, or `read` into a resized tail of `self.buf`); on read of 0 bytes return `Ok(None)` (clean disconnect); on `Err(other)` propagate (poisoned wire — close connection).
  - `BufReader<TcpStream>` is redundant once the frame buffer is on `Session` — drop it or leave it; doesn't affect correctness.
  - Rewrite `Session::execute` to return a `Reply` (no more `format!`). Each `Command` arm builds a `Reply` from the cache result. `Command::Ping` → `Reply::SimpleString(SimpleInner::try_from(b"PONG")?)`. `Command::Get` hit → `Reply::BulkString(bytes)`, miss → `Reply::NullBulk`. `Command::Set` → `Reply::SimpleString(SimpleInner::try_from(b"OK")?)`. `Command::Delete` → `Reply::Integer(0 or 1)`.
  - The repl loop becomes: `read_frame` → `Command::try_from(frame)` → `execute` → `reply.write_to(&mut self.writer)` → `self.writer.flush()`.
  - Smallest end-to-end smoke test: `redis-cli -p 3000 ping` → `PONG`. Then `set foo bar` / `get foo` / `del foo`.
- [ ] Delete bespoke `Command::TryFrom<&str>` and its tests once RESP wiring works. Sunk-cost test surface — the text parser will never run again.
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
