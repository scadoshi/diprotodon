# 05 — The Problems To Solve

The actual plane work. Ordered. Each has the *shape* and the *question to resolve*, not the
answer. Work them in order (see README for the quick walk-through list).

---

## Problem A — Get back to green (mechanical)

Do the rename cascade in `04-compile-cascade.md`. Build. Fix `Cache::execute` callers (the
service still passes a whole `Command` into `cache.execute`, which now wants a
`CacheCommand` — but don't fix that in isolation; it's really Problem B).

**Target:** the non-pub/sub code compiles and the existing ~219 tests pass again.

---

## Problem B — Service routing & the ownership question  *(the crux — decide deliberately)*

`Service::execute` / `execute_logged` take a `Command`. With the three-way enum, they must
now dispatch:
- `Cache(cc)` → `self.cache.execute(cc)`; log the `WriteCommand` if it's a write.
- `Ping{message}` → produce a pong **without touching the cache**.
- `Channel(_)` → ??? The Service has no session id, no sender, no registry handle. It
  *can't* fan out. This is the tension you flagged.

**The open decision: where does Channel routing live?**

- *Option 1:* Service grows a `Channels` field + takes a session id/sender. Cost: pollutes
  the pure cache+persistence orchestrator with transport state; Service is `Clone + Send +
  Sync` shared across sessions and doesn't naturally know "which session is calling."
- *Option 2 (recommended):* the **ReadHalf router** owns pub/sub. It already has `id`,
  `subscriptions`, and `sender`, and it's per-session. It handles `Channel(_)` itself
  (against a shared `Channels` handle it holds), handles `Ping` inline, and forwards only
  `Cache(_)` to the `CacheService`. Keeps Service pure; matches the plan note "the reader
  thread will pass to the CacheService or ChannelService."

If you take Option 2, the reader loop in `repl()` becomes a three-way match instead of the
current blanket `execute_logged(cmd)`. That blanket call is *currently wrong* — it shoves
Channel and Ping commands at the cache service. Fixing that match is the spine of this step.

**Sub-question:** does `Ping` still flow through `CommandOutcome::Pong` (and the existing
reply mapping), or does the router build the pong reply inline? Either works; the former
reuses the tested mapping, the latter keeps Ping out of a cache-flavored enum. Lean former
for less churn.

---

## Problem C — Make `Channels` shareable and reachable

The registry must be **one instance shared by every session**. Today `Channels` holds
`Arc<Mutex<…>>` internally but isn't itself `Clone`, and `subscribe`/`unsubscribe` take
`&mut self`.

- Resolve: with interior mutability via `Arc<Mutex>`, the methods want `&self`, not
  `&mut self` (you can't get `&mut` through a shared handle, and you don't need it). Change
  the receivers.
- Resolve: how does each `ReadHalf` get its handle? Either derive `Clone` on `Channels`
  (cloning the inner `Arc` — cheap, correct) and hand a clone to each session, or wrap the
  whole thing in an outer `Arc<Channels>`. Deriving `Clone` is the lighter touch since the
  `Arc` is already inside.
- Resolve: create the single `Channels` in `server.rs` (next to where the `Service`/cache
  is created) and thread a clone into each `Session` (and thus `ReadHalf`). The `Session`
  constructor / `split()` will need to carry it.

---

## Problem D — Implement the three channel behaviors (in the router)

Each returns a reply to the *issuing* session via its normal reply path (the mpsc).

- **SUBSCRIBE(channel_ids):** for each channel — register `(self.id, self.sender.clone())`
  into `Channels` (the method exists), insert into `self.subscriptions`. Reply **per
  channel** with the array `["subscribe", channel, count]`, where `count` is this session's
  running subscription total *after* adding each. (Redis emits one reply per channel, with
  the cumulative count climbing.)
- **UNSUBSCRIBE(channel_ids):** if `channel_ids` is empty, it means *all* — iterate
  `self.subscriptions`. For each: remove `(channel, self.id)` from the registry, remove from
  `self.subscriptions`. Reply per channel `["unsubscribe", channel, remaining count]`.
  Unsub from a channel you're not in is a no-op (but Redis still sends a confirmation reply
  with the current count — decide if you mirror that nuance now or keep it simple).
- **PUBLISH(channel, message):** lock the registry, look up the channel's `Subscribers`,
  serialize the push `["message", channel, message]` once, and `send()` the bytes into
  every subscriber's `sender`. Prune any sender whose `send` errors (receiver dropped =
  dead session). Reply to the publisher with `:N` where N = subscribers reached.

**Concurrency note:** don't hold the registry `Mutex` across slow work. A `send()` into an
unbounded mpsc is cheap, so holding it across the fan-out loop is fine for v1 — but *know*
that's the call you're making.

---

## Problem E — The reply wire format (array / push variant)

Subscribe/unsubscribe/message are **RESP arrays of mixed elements** (a string, a string,
an integer). Your `Reply` type has no array variant today, and `CommandOutcome` has no
array either.

- Decide: add a `Reply::Array(Vec<Reply>)` variant (general, reused by MULTI/EXEC later —
  the plan explicitly wants this shared) and teach `write_to` / `to_bytes` to emit
  `*<len>\r\n` then each element. The cleaner, reused-once path.

**The two distinct delivery paths — don't conflate them:**
1. *Command replies* (subscribe ack, unsubscribe ack, publish count) go back to the
   issuing session through its own reply flow.
2. *The pushed message* is produced by the **publisher** and serialized straight to bytes,
   then dropped into each **subscriber's** mpsc. It never goes through the subscriber's
   command path or its `CommandOutcome`. It's out-of-band by construction — which is the
   whole reason the writer-thread design works.

So: build the push bytes via the new `Reply::Array` serializer rather than hand-rolling —
one source of truth for RESP array encoding.

---

## Problem F — Disconnect cleanup (don't leak dead senders)

When a session's reader loop ends (EOF/error), its `Sender` clones are still sitting in the
registry under every channel it joined. On the next PUBLISH you'd `send()` into a dead
receiver. Two safety nets, use both:

- **Active:** at the end of `repl()` (after the reader loop breaks), unsubscribe this
  session from all `self.subscriptions`. O(subscribed channels), which is exactly why you
  keep `subscriptions` per session.
- **Passive:** PUBLISH already prunes senders whose `send` errors. That covers the race
  where a session dies mid-publish.

---

## Problem G — Re-home the session tests

The old inline `tests` mod in `session.rs` is commented out — it assumed the
single-threaded `execute`/`writer` shape that no longer exists. Once the `ReadHalf` /
`WriteHalf` API settles, rewrite them against the split. Add coverage for the three channel
commands (subscribe registers, unsubscribe removes, publish fans out + counts, disconnect
cleans up). Don't write these until the API stops moving, or you'll rewrite them twice.
</content>
