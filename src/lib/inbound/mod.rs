//! Inbound layer — everything that turns incoming bytes into a [`crate::domain::command::Command`].
//!
//! - [`server`] — TCP accept loop, thread-per-connection, sweeper/persist/shutdown threads.
//! - [`session`] — the per-connection REPL: reads bytes, parses frames, dispatches commands.
//! - [`resp`] — the wire-protocol parser layer (`Crlf`, `Frame`, `Frame → Command`).

pub mod resp;
pub mod server;
pub mod session;
