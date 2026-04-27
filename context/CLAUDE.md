# diprotodon — Project Context

## Name

The diprotodon was a giant extinct marsupial — basically a hippo-sized wombat that roamed Australia until ~40,000 years ago. The Rust version of this project gets the dignified, sophisticated, ancient-giant name. The Go sibling is `wombat` — diprotodon's goofy modern cousin (literally same suborder, Vombatiformes).

## What this is

A minimal Redis-compatible KV server in Rust. Speaks RESP over TCP. In-memory store with TTL, AOF persistence, and pub/sub.

## Why this exists

This repo is a deliberate interview-prep forcing function for **Scotty Fermo**'s active job applications. It is **private for now** because the context openly references the target companies. It will be **anonymized and made public** once the interview cycles close.

### Target interviews

- **Prax** (Rust) — practical challenge: "build an ergonomic trait" or "extend a small system." This project IS the muscle for that. Most urgent — interview is 1–2 weeks out as of 2026-04-27.
- **Spice AI** (Rust + Go) — pair-programming session in Rust + a separate one in Go. The Rust half is here; the Go half lives in the sibling repo.
- **AURA Network Systems** (Rust, Software Engineer II) — bonus evidence. The JD describes "production-grade Rust software components and services," and this is direct proof. AURA is **not** the driver.

The master study plan lives in `~/Work/appetizers/ideas/redis-mini-study-plan.md`. That file is the source of truth for the trajectory; this `CLAUDE.md` is a pointer to it plus repo-local context.

## Trajectory

Phased milestones (full detail in `context/plan.md`):

- **M0** — TCP echo server (async I/O wired up)
- **M1** — RESP parser + PING/PONG
- **M2** — GET / SET / DEL / EXISTS
- **M3** — EXPIRE / TTL / PEXPIRE (lazy + active sweep)
- **M4** — AOF persistence (append-only log, replay on boot)
- **M5** — Pub/Sub (broadcast fan-out)
- **M6 (stretch)** — RDB snapshots, MULTI/EXEC, Streams, RESP3

Order of attack (per study plan): **Rust M0–M3 first** (Prax pressure), then the Go sibling repo M0–M3 (Spice Go session), then M4–M5 in both as time allows.

## Sibling repo

`~/Work/wombat` — same feature ladder, ported to Go. Don't copy code between them; the *translation* is the point.

## Discipline rules

See `context/rules.md`. The short version:

- **Write by hand:** RESP parser, command dispatch, core data structures, connection loop.
- **AI lane:** boilerplate (Cargo.toml deps, test scaffolding), explanations, debugging help *after* you've read the compiler error yourself.
- **Read compiler errors first.** Always.
- **Commit small.** One feature, one commit.

## Before going public — anonymization checklist

When the interview cycle closes and this repo is ready to be open-sourced:

- Rewrite this `CLAUDE.md` to drop company names (Prax, Spice, AURA) and personal interview context
- Remove or genericize anything in `context/plan.md` that references the interview map
- Scrub commit messages if any reference target companies
- Replace this section with a generic "Background" describing the project as a learning exercise
