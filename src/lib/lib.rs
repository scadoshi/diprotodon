//! diprotodon — a minimal Redis-compatible KV server in Rust.
//!
//! Speaks RESP over TCP. Real `redis-cli` clients connect, ping, get, set, delete,
//! check existence, set TTLs (relative or absolute), query TTLs, and persist keys.
//! In-memory store with disk durability (snapshot baseline + append-only command log),
//! lazy + active expiry, and graceful shutdown.
//!
//! ## Layout
//!
//! Ports-and-adapters (hexagonal). The dependency arrow points inward: adapters depend on
//! the domain, never the reverse.
//!
//! - [`domain`] — wire- and storage-agnostic core: `Cache`, `Entry`, `Command`, the
//!   `Service` orchestrator, and the `ports` (trait boundaries) the adapters plug into.
//!   Nothing here knows about RESP, TCP, or files.
//! - [`inbound`] — driving adapter: TCP accept loop and the per-connection session that
//!   streams RESP in and replies out.
//! - [`outbound`] — driven adapter: the `persister` that gives the cache durability
//!   (append-only log + snapshot) behind the `CacheRepository` port.
//! - [`resp`] — the RESP wire codec (frame parse/encode, replies). Shared by both the
//!   inbound and outbound layers, so it sits beside them rather than inside either.
//!
//! The binary entry point is `src/main.rs`, which just calls
//! [`inbound::server::Server::run`].

pub mod domain;
pub mod inbound;
pub mod outbound;
pub mod resp;

#[cfg(test)]
pub mod test_support;
