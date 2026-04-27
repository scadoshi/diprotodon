# diprotodon — Milestone Plan

Lifted from the master study plan at `~/Work/appetizers/ideas/redis-mini-study-plan.md`. Milestones live here so the project is self-contained; the master plan owns the broader interview-prep trajectory.

## Phase 0 — Pre-study (concepts, ~1 evening)

- TCP server fundamentals: socket → bind → listen → accept → read/write loop
- Concurrency models: thread-per-connection vs event loop vs goroutines vs async tasks
- Read the RESP protocol spec end-to-end: https://redis.io/docs/reference/protocol-spec/
- Skim the Redis command reference for the commands you'll implement

## Phase 1 — Rust refresh (~1 evening)

- tokio fundamentals: TcpListener, TcpStream, AsyncRead/AsyncWrite, spawn
- tokio::sync primitives: mpsc, broadcast, RwLock, Mutex
- Sharing state: Arc<RwLock<HashMap>> vs actor-pattern with mpsc
- Cancellation / graceful shutdown: CancellationToken, signal::ctrl_c
- Reference (don't copy): https://github.com/tokio-rs/mini-redis

## Phase 2 — Build M0–M3 (~2 weekends)

### M0 — TCP echo server (~1 hour)
- TcpListener, accept loop, per-connection task
- Echo back whatever you read

### M1 — RESP parser + PING/PONG (~half day)
- Parse Bulk Strings, Arrays, Simple Strings
- Dispatch one command (PING)
- Hand-write the parser. No nom, no combine, no shortcuts.

### M2 — GET / SET / DEL / EXISTS (~half day)
- In-memory `HashMap<String, Vec<u8>>` behind `Arc<RwLock<...>>`
- Command dispatch table

### M3 — EXPIRE / TTL / PEXPIRE (~half day)
- Track expiry timestamps
- Lazy expiration on read
- Active background sweep (a tokio task ticking on an interval)

## Phase 4 — M4 AOF persistence (~1 weekend)

- Append every write to a log file
- Replay on boot to rebuild state
- Decide flush policy (always vs every-second vs no)

## Phase 5 — M5 Pub/Sub (~1 weekend)

- PUBLISH / SUBSCRIBE
- `tokio::sync::broadcast` is the natural fit
- Fan-out from one publisher to N subscribers

## Phase 6 — Stretch (only if time)

- M6: RDB snapshots, MULTI/EXEC, Streams, RESP3

## Discipline note

Each milestone gets its own commit (or small series). Don't merge milestones. Easier to compare against the Go sibling later.
