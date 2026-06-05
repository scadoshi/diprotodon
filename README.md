# diprotodon

A minimal Redis-compatible in-memory key-value server in Rust. Hand-written from the wire protocol up.

Speaks RESP over TCP. Real `redis-cli` clients can connect, ping, get, set, delete, check key existence, set relative or absolute TTLs, query TTLs, and persist keys against it. Data survives restarts via a snapshot baseline plus an append-only command log. Graceful shutdown via stdin EOF or `quit`/`exit`.

## Why "diprotodon"

The diprotodon was a giant marsupial — basically a hippo-sized wombat — that roamed Australia until ~40,000 years ago. Sophisticated, lumbering, ancient. The Rust version of this project gets the dignified extinct-giant name. The Go sibling (`wombat`) gets the goofy modern-cousin name — same suborder (Vombatiformes), different size.

## Status

| Milestone | State |
| --- | --- |
| M0 — TCP echo server | done |
| M1 — RESP protocol + dispatch | done (PING with optional message echo, framing, error replies) |
| M2 — GET / SET / DEL / EXISTS | done |
| M3 — EXPIRE / EXPIREAT / TTL / PERSIST | done (lazy + active expiry) |
| M4 — AOF persistence | done (snapshot baseline + AOF replay; snapshot-then-truncate compaction) |
| M5 — Pub/Sub | not started |
| Cross-cutting — graceful shutdown | done |

Try it:

```
cargo run
# in another terminal
redis-cli -p 3000 ping
redis-cli -p 3000 ping "hello"
redis-cli -p 3000 set foo bar
redis-cli -p 3000 get foo
redis-cli -p 3000 exists foo
redis-cli -p 3000 expire foo 30
redis-cli -p 3000 expireat foo 1893456000
redis-cli -p 3000 ttl foo
redis-cli -p 3000 persist foo
redis-cli -p 3000 del foo
```

To shut down cleanly, send EOF (Ctrl-D) or type `quit` / `exit` on the server's stdin. The accept loop stops, persistence flushes one last time, and all spawned threads join before `main` returns.

## What's interesting in here

- **Hand-written RESP parser** with proper streaming semantics. The parser is the framer — it returns `Incomplete` when bytes are short, `Malformed` when bytes are wrong, and `Ok((frame, leftover))` otherwise. The leftover slice borrows from the input — no allocation for the rest-of-the-buffer.
- **Binary safety end-to-end.** Keys and values are `Vec<u8>`, not `String`. Bulk-string payloads on the wire can be arbitrary bytes (jpegs, interior `\r\n`, whatever). UTF-8 is never enforced where the protocol doesn't require it.
- **Newtype validation for protocol invariants.** `SimpleInner` guarantees no `\r`/`\n` in simple-string and simple-error payloads at construction time. Trusted constructors (`ok()`, `pong()`) and a sanitizing constructor (`sanitized(...)`) for arbitrary error message bytes.
- **Hexagonal (ports & adapters).** The domain defines two trait *ports* — `CacheRepository` (driven/outbound, implemented by the persister) and `CacheService` (driving/inbound, implemented by the domain `Service` and called by the session). The dependency arrow points inward: adapters depend on the domain, never the reverse. Adapter errors are mapped into a domain-owned `RepositoryError` *at the boundary*, so the domain never names an outbound type.
- **AOF is the wire protocol.** The append-only log stores each mutating command as the exact RESP bytes a client would have sent (same `From<MutatingCommand> for Frame` + `Frame::write_to` as the network path). So replay needs no special decoder — it reuses `Frame::parse_one` + `Command::try_from`, the inbound parsing path. Relative `EXPIRE` is normalized to an absolute `EXPIREAT` before logging so replay is time-invariant. Recovery loads the snapshot, then replays the log on top; a trailing torn frame (crash mid-append) stops replay cleanly, while mid-stream corruption or an unknown command is fatal. Snapshots are written atomically (temp file + `rename`) and truncate the log — that's the compaction.
- **Generic Session.** `Session<R: Read, W: Write, CS: CacheService>` — the connection loop can be tested with `Cursor<Vec<u8>>` instead of a real TCP socket, and against a fake service instead of a real cache. It's a pure streamer: RESP framing in, outcome→reply translation out, all execution behind the service.
- **Iterative array parsing.** Recursive `parse_array` would blow the stack on `MGET key1..key1000`. Iterative loop with a Vec is one extra concept and zero stack-overflow risk.
- **TTL design.** Unified `Entry { value: Vec<u8>, absolute_ttl: Option<u64> }` (absolute UNIX seconds) — simpler than a sidecar map and the perf gain from sidecar isn't worth the complexity at this scale. `SystemTime`-based absolute seconds on disk because `Instant` is process-local and unserializable by design. EXPIRE (relative seconds-from-now) feeds through the same absolute representation; EXPIREAT (absolute UNIX seconds) sets it directly — a past timestamp deletes immediately and returns `1`, matching real Redis. Lazy expiry on every read path (`get`, `contains`, `get_absolute_ttl`) drops expired keys on access; a background sweeper thread does active eviction on a 10s tick.
- **Graceful shutdown.** `Arc<AtomicBool>` shutdown flag. The listener is set non-blocking with a 50ms throttle on `WouldBlock` so the accept loop polls the flag without burning CPU. Stdin EOF / `quit` / `exit` flip the flag — no `ctrlc` crate dependency. Every spawned thread is collected as a `JoinHandle` and joined cleanly before `main` returns; the persistence and sweeper threads check the flag on a 100ms-tick budget so shutdown latency stays bounded.
- **Error-path test coverage.** Every command's parser arm has explicit coverage for `TooManyParts`, `NotEnoughParts`, `UnexpectedFrame`, and (where applicable) `Utf8` / `ParseInt` failures on numeric args.

## Layout

```
src/
  main.rs                  # binary entry
  lib/
    lib.rs                 # module roots
    domain/                # wire- and storage-agnostic core
      cache.rs             # Arc<Mutex<HashMap<Vec<u8>, Entry { value, absolute_ttl }>>>, lazy expiry, sweep
      command.rs           # Command + MutatingCommand (logged subset) + CommandOutcome / TtlOutcome
      ports.rs             # CacheRepository (outbound) + CacheService (inbound) traits, domain errors
      service.rs           # Service<CR> — orchestrates cache execution + AOF append
    inbound/               # driving adapter
      server.rs            # TCP accept loop, thread-per-connection, sweeper + persistence + shutdown threads
      session.rs           # per-connection streamer + SessionReader (frame buf); CommandOutcome -> Reply
    outbound/              # driven adapter
      persister/
        mod.rs             # Persister — CacheRepository impl composing aof + snapshot
        aof.rs             # append-only command log: append / replay / clear
        snapshot.rs        # wincode snapshot: load / store (atomic temp + rename)
        persister_inner.rs # shared writer-handle + path plumbing
    resp/                  # shared RESP codec (used by inbound AND outbound)
      crlf.rs              # Crlf trait on [u8] — is_crlf / split_crlf
      frame.rs             # Frame::parse_one (decode) + write_to (encode) + From<MutatingCommand>
      command.rs           # TryFrom<Frame> for Command
      reply.rs             # Reply enum + write_to + SimpleInner newtype
```

## Sibling

A Go port of the same feature ladder lives in the `wombat` repo. Same family (Vombatiformes — diprotodon and modern wombats share a suborder), different language, different lessons. The translation between them is the point; code is not copied across.

## Development context

`context/` holds the project's design notes — plan and milestone status, RESP working notes, commit guidelines, discipline rules. Useful if you're poking around the architecture or picking up where I left off.
