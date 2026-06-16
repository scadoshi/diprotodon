# 02 — What's Already Built (Review)

Trust but verify — run `cargo build` and let the compiler confirm. (It won't fully build
yet; see `04-compile-cascade.md`.)

## Command model — DONE and coherent
- `Command::{Cache(CacheCommand), Channel(ChannelCommand), Ping{message}}` — top-level
  three-way. `From` impls + thin constructor shims (`Command::get`, `::set`, `::subscribe`,
  `::publish`, …) preserve every existing call site and test.
- `CacheCommand::{Read(ReadCommand), Write(WriteCommand)}` — the projection. `WriteCommand`
  is the **renamed `MutatingCommand`** (now at `domain/command/cache/write.rs`).
- `ChannelCommand::{Subscribe{channel_ids: Vec<Vec<u8>>}, Unsubscribe{channel_ids:
  Vec<Vec<u8>>}, Publish{channel_id: Vec<u8>, message: Vec<u8>}}`. Subscribe/Unsubscribe
  are multi-channel; Publish is single-channel (Redis-standard). Derives `Clone, PartialEq`.
- `CommandError` lives at the top level; the old `CacheCommandError` was deleted as redundant.

## RESP parsing — DONE
- `resp/command.rs` parses `subscribe` / `unsubscribe` / `publish` verbs into the new
  shapes. subscribe requires ≥1 channel; unsubscribe allows zero args (means "all");
  publish is channel + message, arity-checked. Existing GET/SET/etc arms untouched.

## Cache execution — DONE
- `Cache::execute` now takes a `CacheCommand` and does a two-level match
  (`Read(R::Get{..})` / `Write(W::Set{..})`). Tests use local helpers that build
  `CacheCommand` directly.

## channels.rs — PARTIALLY built (the registry skeleton)
- `Subscriber { id: u32, sender: Sender<Vec<u8>> }` — a session's identity + its outbound
  mpsc handle.
- `Subscribers` — newtype over `HashMap<u32, Sender<Vec<u8>>>` (Deref/DerefMut). Keyed by
  **session id** so unsubscribe and disconnect-cleanup are O(1), not a scan.
- `Channels { channels: Arc<Mutex<HashMap<Vec<u8>, Subscribers>>> }` with `subscribe` and
  `unsubscribe` methods. **The shared registry already owns its `Arc<Mutex>` internally.**
- `ChannelsError::MutexPoisoned` exists.

## session.rs — split, but routing is stale
- `Session` splits into `ReadHalf` (id, reader, cache_service, sender, subscriptions) and
  `WriteHalf` (writer, receiver) over a per-session `mpsc<Vec<u8>>`. `repl()` spawns the
  writer thread (drains mpsc → socket) and runs the reader on the current thread. Verified
  end-to-end for the cache commands.
- `ReadHalf.id` and `ReadHalf.subscriptions` are **dormant** (the two dead_code warnings).
  They wake up when SUBSCRIBE/UNSUBSCRIBE land.
</content>
