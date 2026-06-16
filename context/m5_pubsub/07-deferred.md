# 07 — Deliberately Deferred

NOT plane work. Resist scope creep — these are recorded so you can *stop* thinking about
them, not start.

- **Pattern subs** (`PSUBSCRIBE` / `PUNSUBSCRIBE`) and the `pmessage` wire shape.
- **Subscribed-mode command restriction** (reject non-pubsub commands while a session is
  subscribed).
- **Slow-subscriber backpressure** (drop/disconnect past a buffer limit). Unbounded for now.
- **Async/tokio migration.** Still owed, still *after* this.
- **`numsub` / `channels` introspection commands.**
</content>
