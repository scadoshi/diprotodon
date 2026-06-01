//! Domain types — the wire-agnostic heart of the server.
//!
//! Two modules:
//!
//! - [`cache`] — the in-memory [`cache::Cache`] (Arc-shared KV store with TTL, lazy +
//!   active expiry, and snapshot persistence).
//! - [`command`] — the parsed [`command::Command`] enum and its [`command::CommandError`],
//!   produced by the RESP layer and consumed by the session executor.
//!
//! Nothing in here knows about TCP, RESP, or any specific wire format. That's the point:
//! the inbound layer parses bytes into a `Command`, the outbound layer serializes a
//! reply, and the cache only ever sees domain types. Keeping the boundary tight is what
//! lets the same `Cache` and `Command` types be reused if the wire protocol changes.

pub mod cache;
pub mod command;
