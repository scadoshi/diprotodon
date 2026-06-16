# 03 — Decisions Already Locked

Don't relitigate these on the plane. They're settled; build on them.

1. **Three-way Command, with Cache projected into Read/Write.** Rationale: type-safety,
   the AOF only logs writes, and non-cache commands don't pollute `Cache::execute`.
2. **Subscribers keyed by session id** (`HashMap<u32, Sender>`), not a `Vec<Sender>`. Gives
   O(1) unsubscribe and clean disconnect removal.
3. **UNSUBSCRIBE from a channel you're not in = no-op**, not an error. Matches Redis.
4. **PUBLISH is single-channel.** Redis-standard.
5. **Reuse the existing per-session mpsc + WriteHalf as the subscriber output path.** A
   published message is just bytes pushed into each subscriber's existing channel — the
   writer thread that already drains it *is* the fan-out delivery thread. No new
   per-subscription thread.
6. **Sync-first.** Async/tokio migration stays deferred. Feel the threading model first.
7. **Slow-subscriber policy: unbounded queue for v1.** Don't build drop/disconnect yet.
8. **Subscribed-mode command restriction: deferred.** Don't gate commands by mode yet.
9. **Pub/sub routing is a session/transport concern, not a Service concern.** This is the
   one genuinely open architectural call — `05-problems.md` Problem B gives the
   recommendation and the trade-off, but make it deliberately.
</content>
