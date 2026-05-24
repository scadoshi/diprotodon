# diprotodon

A minimal Redis-compatible in-memory key-value server in Rust. Hand-written from the wire protocol up.

Speaks RESP over TCP. Real `redis-cli` clients can connect, ping, get, set, delete, and check key existence against it.

## Why "diprotodon"

The diprotodon was a giant marsupial — basically a hippo-sized wombat — that roamed Australia until ~40,000 years ago. Sophisticated, lumbering, ancient. The Rust version of this project gets the dignified extinct-giant name. The Go sibling (`wombat`) gets the goofy modern-cousin name — same suborder (Vombatiformes), different size.

## Status

| Milestone | State |
| --- | --- |
| M0 — TCP echo server | done |
| M1 — RESP protocol + dispatch | done (PING, framing, error replies) |
| M2 — GET / SET / DEL / EXISTS | done |
| M3 — EXPIRE / TTL | not started |
| M4 — AOF persistence | snapshot exists; AOF is the upgrade target |
| M5 — Pub/Sub | not started |

Try it:

```
cargo run
# in another terminal
redis-cli -p 3000 ping
redis-cli -p 3000 set foo bar
redis-cli -p 3000 get foo
redis-cli -p 3000 exists foo
redis-cli -p 3000 del foo
```

## What's interesting in here

- **Hand-written RESP parser** with proper streaming semantics. The parser is the framer — it returns `Incomplete` when bytes are short, `Malformed` when bytes are wrong, and `Ok((frame, leftover))` otherwise. The leftover slice borrows from the input — no allocation for the rest-of-the-buffer.
- **Binary safety end-to-end.** Keys and values are `Vec<u8>`, not `String`. Bulk-string payloads on the wire can be arbitrary bytes (jpegs, interior `\r\n`, whatever). UTF-8 is never enforced where the protocol doesn't require it.
- **Newtype validation for protocol invariants.** `SimpleInner` guarantees no `\r`/`\n` in simple-string and simple-error payloads at construction time. Trusted constructors (`ok()`, `pong()`) and a sanitizing constructor (`sanitized(...)`) for arbitrary error message bytes.
- **Hexagonal layout.** `src/lib/{domain,inbound,outbound}/` — wire-protocol code is one-way coupled to domain types, not the other way around.
- **Generic Session.** `Session<R: Read, W: Write>` so the connection loop can be tested with `Cursor<Vec<u8>>` instead of a real TCP socket.
- **Iterative array parsing.** Recursive `parse_array` would blow the stack on `MGET key1..key1000`. Iterative loop with a Vec is one extra concept and zero stack-overflow risk.

## Layout

```
src/
  main.rs              # binary entry
  lib/
    lib.rs             # module roots
    domain/
      cache.rs         # Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>
      command.rs       # Command enum (GET/SET/DEL/EXISTS/PING)
    inbound/
      server.rs        # TCP accept loop, thread-per-connection
      session.rs       # per-connection REPL + SessionReader (frame buf)
      resp/
        crlf.rs        # Crlf trait on [u8] — is_crlf / split_crlf
        frame.rs       # Frame::parse_one — the parser/framer
        command.rs     # TryFrom<Frame> for Command
    outbound/
      resp/
        reply.rs       # Reply enum + write_to + SimpleInner newtype
```

## Sibling

A Go port of the same feature ladder lives in the `wombat` repo. Same family (Vombatiformes — diprotodon and modern wombats share a suborder), different language, different lessons. The translation between them is the point; code is not copied across.

## Development context

`context/` holds the project's design notes — plan and milestone status, RESP working notes, commit guidelines, discipline rules. Useful if you're poking around the architecture or picking up where I left off.
