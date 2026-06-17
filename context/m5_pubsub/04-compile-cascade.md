# 04 — The Compile Cascade Is Currently BROKEN

You renamed `MutatingCommand → WriteCommand` and moved it, and changed `Cache::execute`'s
signature — but the downstream callers still speak the old language. `cargo build` fails
until these are reconciled. Knock them out first: they're mechanical and they get you back
to green so the *interesting* work compiles as you go. (This is Problem A in
`05-problems.md`.)

## Files still referencing the old `MutatingCommand` name/path

Grep-confirmed:

- `domain/service.rs` — import + `MutatingCommand::from_command` + tests
- `domain/ports.rs` — import + `append(mutating_command: MutatingCommand)` signature
- `resp/frame.rs` — import + `type MC = MutatingCommand` + `From<MutatingCommand> for Frame`
- `test_support.rs` — `RecordingRepo.appended: Vec<MutatingCommand>` + `append`
- `outbound/persister/aof.rs` — import + `append` signature + many test constructors
- `outbound/persister/mod.rs` — import + `append` signature + tests

## The mechanical part

Rename to `WriteCommand`, re-path imports to
`crate::domain::command::cache::write::WriteCommand`. The variants (`Set/Delete/Expire/
ExpireAt/Persist`) are identical, so test bodies only need the type name swapped.

## The one non-mechanical bit

`MutatingCommand::from_command(Command)` used to *classify* a flat command into "is this a
mutation?". Now the type system already answers that — a write is precisely
`Command::Cache(CacheCommand::Write(w))`. So the classifier collapses into a match that
pulls the `WriteCommand` out when present and returns `None` otherwise.

Question to resolve: does this want to be a free function, a `TryFrom<Command> for
WriteCommand`, or an inherent `Command::as_write(&self) -> Option<&WriteCommand>`? Pick
based on who calls it (the service, on the logging path) and whether you want to clone or
borrow.
</content>
