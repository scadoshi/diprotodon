//! Outbound layer — the driven adapters the domain reaches through its ports.
//!
//! Currently just [`persister`], the
//! [`CacheRepository`](crate::domain::ports::CacheRepository) implementation that gives
//! the cache durability on disk (append-only command log + periodic snapshot). The RESP
//! wire codec lives in the shared [`resp`](crate::resp) module rather than here, because
//! both the inbound and outbound layers use it.

pub mod persister;
