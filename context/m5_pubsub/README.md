# M5 Pub/Sub — Plane Review Pamphlet

Offline coding companion. No internet, no AI. This directory is your map for finishing
pub/sub on the plane. Walk the files in number order — each one has a single job.

> Discipline reminder (from `rules.md`): read the compiler error first, write the core
> by hand, commit small. These notes give you *shapes and questions*, not answers. The
> point is the muscle.

---

## How to read this

| File | What it's for |
|------|---------------|
| `01-mental-model.md`    | The one diagram + the three concerns to keep separate. Read first. |
| `02-whats-built.md`     | Review of what already exists. Trust but verify with `cargo build`. |
| `03-locked-decisions.md`| Calls you already made. Don't relitigate these at 35,000 ft. |
| `04-compile-cascade.md` | Why the branch doesn't compile right now + the mechanical fix. |
| `05-problems.md`        | The actual build work: Problems A–G, ordered, with the shape of each. |
| `06-traps.md`           | The bugs that *will* bite. Skim before coding, re-read when stuck. |
| `07-deferred.md`        | Explicitly NOT plane work. Resist scope creep. |

---

## Suggested order on the plane

The full reasoning for each is in `05-problems.md`. The walk-through:

1. **Problem A** — rename cascade (`MutatingCommand → WriteCommand`), get to green. Pure
   mechanical; you can't test anything else until it builds.
2. **Problem B** — decide ownership (recommend the ReadHalf router), make `repl()`'s reader
   loop a three-way router, get Ping + Cache flowing correctly through it.
3. **Problem C** — make `Channels` `Clone` + `&self`, create it once in `server.rs`, thread
   it into sessions.
4. **Problem E (array variant)** — build `Reply::Array` + serializer *before* D's replies
   mean anything. Test the bytes in isolation — lean on your reply-bytes test tradition.
5. **Problem D** — implement subscribe, then publish, then unsubscribe.
6. **Problem F** — disconnect cleanup.
7. **Problem G** — re-home/extend tests once the API stops moving.

---

## Offline smoke test

Two terminals, both `redis-cli -p 3000`:

- Terminal 1: `SUBSCRIBE foo`
- Terminal 2: `PUBLISH foo bar`

Terminal 1 should print the message; the publisher should get `(integer) 1`. This is your
end-to-end proof — works with no internet.

---

## One-paragraph orientation (if you forget everything else)

Three-way `Command` is done and parses. The cache path works. The job is: (1) finish the
`MutatingCommand→WriteCommand` rename so it compiles, (2) turn the session reader loop into
a router that sends cache commands to the `CacheService`, handles `Ping` inline, and
handles `Channel` commands against a shared `Channels` registry it holds a clone of, (3)
add a `Reply::Array` variant for the RESP-array replies pub/sub needs (reused by EXEC
later), (4) implement subscribe/unsubscribe/publish where publish serializes a `["message",
channel, payload]` array and drops the bytes into each subscriber's existing mpsc — the
writer thread already delivers them — and (5) clean up the registry on disconnect. Keep the
three concerns — cache, registry, transport — from leaking into each other.
</content>
