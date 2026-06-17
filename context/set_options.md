# SET options — build plan

Adding option support to `SET`: `SET key value EX 60` and friends. Rough high-level
notes + boundary map + current state.

> **Status: WIP, does NOT compile.** Mid-rename `SetOptions` → `SetExpiry` (see "Current
> state"). The EXPIRE→EXPIREAT persistence fix that this work grew out of is **done and
> tested**; the SET arm itself is still scaffolding.

## Scope

- **Now:** `EX` / `EXAT` (seconds). Parse + execute + AOF + tests.
- **Soon:** `PX` / `PXAT` (millis) — gated on the units decision below.
- **Later:** `KEEPTTL`, `NX` / `XX`, `GET`. Plain `SET` (no option) must clear any existing
  TTL — that's the default Redis behavior and the baseline to preserve.

## Locked decision: absolute-only command type, normalize relative → absolute at PARSE

Relative TTLs never live as a command. The wire `EX`/`PX` (and `EXPIRE`) are converted to an
absolute deadline (`now + ttl`) **at parse time**, and only the absolute form exists in the
command type. So `SetExpiry` is **absolute-only**: `{ ExAt(u64), PxAt(u64) }` — same move as
killing `WriteCommand::Expire` in favor of `ExpireAt`.

**Why parse-time is safe here (this supersedes the earlier "never in the parser" note):**
the rule that actually matters is *the replay path must be time-invariant*. `Command::try_from`
is shared by the live network path AND AOF replay — but because the relative form is
**unrepresentable** in the command type, it can never be logged. So the AOF only ever holds
absolute (`EXPIREAT` / `SET … EXAT`), replay only ever hits the absolute parse arms, and the
clock read in the relative arms (`EXPIRE`, `SET … EX`) runs for **live input only, never
replay**. Removing the relative variant is what makes clock-in-parser safe.

Consequences: one `now()` read per command; cache and log agree by construction;
`From<WriteCommand> for Frame` stays pure (serializes an already-absolute value);
`set_relative_ttl` is now effectively dead.

Apply the identical pattern to `EXPIRE` and `SET … EX` so they never drift. (EXPIRE already does.)

## EXPIRE→EXPIREAT fix — DONE (reference implementation for SET … EX)

The original bug: `From<WriteCommand> for Frame` logged `EXPIRE` verbatim (relative), so replay
re-applied `now_at_replay + ttl` and deadlines drifted on every restart.

Fixed this session:
- Removed `WriteCommand::Expire` entirely (variant + constructors + `Command::expire`). Only
  `ExpireAt` remains.
- `resp/command.rs` `b"expire"` arm reads the clock and emits `ExpireAt(now + ttl)` via
  `saturating_add` (overflow-safe).
- Tests: `try_from_frame_ok_expire_normalizes_to_absolute` (tolerance-bracketed),
  `try_from_frame_err_expire_too_many_parts`, and `aof::replay_applies_expire_at` (proves an
  EXPIREAT survives the log round-trip; seeds the key directly since SET→Frame is still a todo).

`SET … EX` is the same shape: parse → `now + ttl` → `SetExpiry::ExAt` → log absolute.

## Current state (per file)

- `command/cache/write.rs` — **done.** `SetExpiry { ExAt(u64), PxAt(u64) }` (absolute-only);
  `WriteCommand::Set { key, value, options: Option<SetExpiry> }`; `set()` ctor updated.
- `resp/command.rs` `b"set"` arm — **scaffolding / todo.** Grabs the next two tokens
  (`set_expiry`, `relative_ttl`) but doesn't parse them yet. Owes: match the keyword
  (EX/PX/EXAT/PXAT, case-insensitive), parse the number, normalize EX/PX → `ExAt`/`PxAt`,
  build the option, arity + error handling. (The clock read + `saturating_add` mirror the
  `b"expire"` arm right above it.)
- `domain/cache.rs` `execute` Set arm — **BROKEN + todo.** Still imports the old `SetOptions`
  (line 27) and matches `SetOptions::Ex/Px/ExAt/PxAt` (line ~302) — stale after the rename, so
  this is a compile error. Owes: switch to `SetExpiry::{ExAt, PxAt}`, apply the TTL (fold to one
  `insert(key, Entry::new(value, abs))`).
- `resp/frame.rs` `From<WriteCommand> for Frame` Set arm — **todo** (`todo!("handle set options
  here")`). Owes: serialize SET with its absolute option (e.g. `SET key val EXAT <n>`).
- `domain/command/mod.rs` — **BROKEN.** Still imports `SetOptions` (line 12) and `Command::set`
  takes `Option<SetOptions>` (line ~70). Rename to `SetExpiry`.

**The two compile errors are both the incomplete rename** (`SetOptions` → `SetExpiry`) in
`cache.rs` and `mod.rs`. Finishing the rename gets back to green (todos still panic if hit).

## Open decision: PX / PXAT units

`SetExpiry::PxAt` carries **milliseconds**, but `Entry.absolute_ttl` is UNIX **seconds**. No
faithful home for sub-second precision today:
- (a) Ship `EX`/`EXAT` now; leave `PX`/`PXAT` as `todo!()`. (preferred — don't let units block EX)
- (b) Lossy ms → s truncation.
- (c) Widen `Entry` to millis (ripples into TTL reply, sweeper, EXPIREAT).

## Future shape (KEEPTTL / NX / XX / GET)

`SetExpiry` is one axis (mutually exclusive — enum is correct; `KEEPTTL` slots in as a variant).
`NX`/`XX` and `GET` are orthogonal. When they land:
- `enum SetExpiry { ExAt, PxAt, KeepTtl }`
- `enum Existence { Nx, Xx }`
- `struct SetOptions { expiry: Option<SetExpiry>, existence: Option<Existence>, get: bool }`
  (the `Set` field becomes this struct; `SetExpiry` stays the expiry sub-axis).

## Boundary map

| Layer | File | Job | Purity |
|---|---|---|---|
| Parse + normalize | `resp/command.rs` | tokens → `Set { …, options }`; relative EX/PX → absolute (`now + ttl`, saturating); validate combos / missing / non-numeric / too-many | reads clock, **live-only** (safe: relative never logged) |
| Apply | `cache.rs::execute` Set arm | compute `Option<u64>` abs once → single `insert(key, Entry::new(value, abs))`; fresh Entry clears prior TTL (correct default) | — |
| Encode | `resp/frame.rs` `From<WriteCommand> for Frame` | serialize already-absolute option | **pure** — no `now()` |

## Resume order

1. **Finish the rename** (`SetOptions` → `SetExpiry`) in `cache.rs` + `mod.rs` → back to green.
2. `command.rs` `b"set"` arm: parse + normalize (un-fails `try_from_frame_err_set_too_many_parts`).
3. `cache.rs` Set arm: apply the TTL.
4. `frame.rs` Set arm: encode with options.
5. Resurrect SET tests: `command.rs` (add EX happy path + option errors), `service.rs`,
   `cache.rs`, `aof.rs` (the commented Set batch + restore the SET-then-EXPIREAT form of
   `replay_applies_expire_at`).
