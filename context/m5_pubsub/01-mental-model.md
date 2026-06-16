# 01 — The Mental Model

Read this first. Everything else hangs off it.

## The pipeline

```
wire bytes ──► Frame ──► Command ──┬─ Cache(CacheCommand)  ──► cache + AOF   (the old path, works)
                                   ├─ Channel(ChannelCommand) ──► pub/sub registry  (NEW, unbuilt)
                                   └─ Ping{message}        ──► inline pong       (NEW routing)
```

## The big idea you committed to

**`Command` is a three-way enum now.** Cache commands are the only ones that touch the
store, and they're further split into `Read` / `Write` so the AOF only ever has to think
about `Write`. Channel commands and Ping never touch the cache — which is exactly why
forcing `Cache::execute` to grow arms for them felt wrong, and why you split them out.

## The three concerns — keep them from leaking

Hold this distinction the whole time. Most bugs on this branch come from one of these
bleeding into another:

1. **The cache** — the in-memory store + AOF. Only `Cache(CacheCommand)` touches it.
2. **The subscriber registry** (`Channels`) — who's subscribed to what. Only `Channel`
   commands touch it.
3. **The session transport** — the per-session mpsc + writer thread that moves bytes to
   the socket. Both command replies *and* out-of-band pushed messages ride this.

If you ever find yourself reaching for a session id inside the cache, or a `Channels`
handle inside `Service`, stop — you're leaking a concern. See `05-problems.md` Problem B,
where this exact tension is the central design decision.
</content>
