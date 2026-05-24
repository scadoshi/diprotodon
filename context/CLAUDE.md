# diprotodon — Project Context

## Name

The diprotodon was a giant extinct marsupial — basically a hippo-sized wombat that roamed Australia until ~40,000 years ago. The Rust version of this project gets the dignified, sophisticated, ancient-giant name. The Go sibling is `wombat` — diprotodon's goofy modern cousin (literally same suborder, Vombatiformes).

## What this is

A minimal Redis-compatible KV server in Rust. Speaks RESP over TCP. In-memory store, with TTL, AOF persistence, and pub/sub on the roadmap.

Built by hand to rebuild Rust async/concurrency/protocol muscle. The point is the muscle, not shipping a product — shortcuts that skip the muscle defeat the project.

## Trajectory

Phased milestones (full detail in `context/plan.md`):

- **M0** — TCP echo server (sync threads first; async migration owed)
- **M1** — RESP parser + PING/PONG
- **M2** — GET / SET / DEL / EXISTS
- **M3** — EXPIRE / TTL / PEXPIRE (lazy + active sweep)
- **M4** — AOF persistence (append-only log, replay on boot)
- **M5** — Pub/Sub (broadcast fan-out)
- **M6 (stretch)** — RDB snapshots, MULTI/EXEC, Streams, RESP3

## Sibling repo

`~/Projects/wombat` — same feature ladder, ported to Go. Don't copy code between them; the *translation* is the point.

Related: `~/Projects/nighthawk` is a separate LSM-style storage engine (WAL + memtable + SSTable + bloom filters + compaction) in Rust. Diprotodon is deliberately *not* LSM — Redis is in-memory-first and persistence is durability, not storage. Different design center, different project.

## Discipline rules

See `context/rules.md` — the **AI collaboration mode** section at the top of that file is mandatory reading for any AI assistant.

The short version:

- **Write by hand:** RESP parser, command dispatch, core data structures, connection loop.
- **AI lane:** boilerplate (Cargo.toml deps, test scaffolding), explanations, debugging help *after* you've read the compiler error yourself.
- **AI mode:** guide educationally. No straight answers, no code in `.rs` files. Lead with questions. Affirm correctness when it's right; don't push the user toward unnecessary churn.
- **Read compiler errors first.** Always.
- **Commit small.** One feature, one commit.
