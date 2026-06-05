//! Domain types — the wire- and storage-agnostic heart of the server.
//!
//! - [`cache`] — the in-memory [`cache::Cache`] (Arc-shared KV store with TTL and
//!   lazy + active expiry). Pure in-memory; durability lives behind the persister.
//! - [`command`] — the parsed [`command::Command`] enum and its [`command::CommandError`],
//!   produced by the RESP layer and consumed by the service executor.
//! - [`ports`] — trait boundaries (`CacheRepository`, `CacheService`) the adapters plug
//!   into. The domain depends only on these; concrete adapters live in `inbound` /
//!   `outbound`.
//! - [`service`] — the [`service::Service`] orchestrator that composes a [`cache::Cache`]
//!   with a [`ports::CacheRepository`] to execute commands (and optionally log them).
//!
//! Nothing in here knows about TCP, RESP, or files. The inbound layer parses bytes into a
//! `Command`, the outbound persister appends mutations and snapshots, and the cache only
//! ever sees domain types. Keeping the boundary tight is what lets the wire format or the
//! persistence strategy change without touching the core.

pub mod cache;
pub mod command;
pub mod ports;
pub mod service;
