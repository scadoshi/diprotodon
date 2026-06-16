# 06 — Traps To Watch

These will bite. Skim before coding; re-read when something's mysteriously wrong.

- **`CommandOutcome::Pong` orphaned.** Ping left the cache enum but `CommandOutcome` still
  has `Pong`. Make sure *something* still produces it (the router), or rip it out and build
  the pong reply inline. Don't leave a variant nothing constructs.

- **The blanket `execute_logged(cmd)` in `repl()` is wrong now.** It routes Channel and
  Ping at the cache service. This is the first thing the three-way router fixes
  (`05-problems.md` Problem B).

- **`&mut self` on `Channels` vs shared handle.** You can't call a `&mut self` method
  through a shared/cloned `Channels`. Switch to `&self` (interior mutability covers it) or
  you'll fight the borrow checker the whole way.

- **Holding the registry lock across the fan-out.** Fine for v1 (sends are cheap), but be
  *deliberate* — note it, don't stumble into it.

- **Subscribe reply count semantics.** It's the *cumulative* count of this session's
  subscriptions, emitted once per channel as you add them — not a constant. Easy to get
  subtly wrong.

- **Self-publish.** If a session is subscribed to `foo` and also publishes to `foo`, it
  receives its own message (Redis does deliver it). Your id-keyed registry already does
  this naturally — just don't add a "skip self" you didn't need.

- **Two reply paths.** Command acks vs out-of-band pushes are different flows
  (`05-problems.md` Problem E). Keep them separate in your head or you'll try to route a
  push through `CommandOutcome`.
</content>
