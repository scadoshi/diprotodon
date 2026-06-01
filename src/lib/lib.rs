//! diprotodon — a minimal Redis-compatible KV server in Rust.
//!
//! Speaks RESP over TCP. Real `redis-cli` clients connect, ping, get, set, delete,
//! check existence, set TTLs (relative or absolute), query TTLs, and persist keys.
//! In-memory store with snapshot persistence, lazy + active expiry, and graceful
//! shutdown.
//!
//! ## Layout
//!
//! - [`domain`] — wire-agnostic types: `Cache`, `Entry`, `Command`. The heart of the
//!   server; nothing here knows about RESP or TCP.
//! - [`inbound`] — TCP accept loop, per-connection session, RESP parsing.
//! - [`outbound`] — reply serialization back onto the wire.
//!
//! The binary entry point is `src/main.rs`, which just calls
//! [`inbound::server::Server::run`].

pub mod domain;
pub mod inbound;
pub mod outbound;
