//! Domain ports — the trait boundaries the rest of the system plugs into.
//!
//! Both traits are *defined here, in the domain*, so the dependency arrow points inward:
//! adapters depend on the domain, never the reverse. The domain names the capabilities it
//! needs and the errors it cares about; outbound adapters implement [`CacheRepository`]
//! and inbound adapters drive [`CacheService`].
//!
//! - [`CacheRepository`] — **driven (outbound) port.** Implemented by the persistence
//!   adapter, called by the service. The domain says "I need to durably log mutations and
//!   snapshot state"; the adapter satisfies it.
//! - [`CacheService`] — **driving (inbound) port.** Implemented by the domain
//!   [`Service`](crate::domain::service::Service), called by the inbound session.
//!
//! The error types are domain-owned too. Adapters map their concrete failures into this
//! vocabulary *at the boundary* (the `From<…> for RepositoryError` impls in the outbound
//! layer), so the domain never names an outbound error type.

use crate::domain::{
    cache::{Cache, CacheError},
    command::{Command, CommandOutcome, MutatingCommand},
};
use thiserror::Error;

/// Failure crossing the persistence boundary, expressed in terms the *domain* can react
/// to rather than the adapter's concrete failure taxonomy.
///
/// Right now there's a single opaque [`Generic`](RepositoryError::Generic) catch-all: the
/// domain doesn't yet branch on persistence failure modes, so every adapter error is
/// boxed into it. Semantic variants (e.g. `Unavailable`, `Corrupt`) get carved out only
/// when the service actually needs to decide on one — not speculatively.
#[derive(Debug, Error)]
pub enum RepositoryError {
    /// Opaque catch-all. The adapter boxes any error it has no domain meaning for into
    /// here; the domain treats it as "persistence failed" without inspecting the cause.
    #[error(transparent)]
    Generic(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Outbound (driven) persistence port. Implemented by the persistence adapter; the
/// service depends on this trait, not on any concrete persister.
///
/// The `Clone + Send + Sync + 'static` bounds let one repository handle be shared across
/// every session thread (each session holds a clone) and live for the whole process.
pub trait CacheRepository: Clone + Send + Sync + 'static {
    /// Durably log one state-mutating command (append to the write-ahead log).
    fn append(&self, mutating_command: MutatingCommand) -> Result<(), RepositoryError>;
    /// Snapshot the current cache state to durable storage (and compact the log).
    fn snapshot(&self, cache: &Cache) -> Result<(), RepositoryError>;
}

/// Failure from the service layer — a union of the two things a command touches.
///
/// `execute` only hits the cache, so it can only fail with [`Cache`](ServiceError::Cache).
/// `execute_logged` also appends to the repository, so it can additionally fail with
/// [`Repository`](ServiceError::Repository). Both variants are domain-owned; the `#[from]`
/// conversions let `?` lift either underlying error at the call site.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Cache operation failed during execution.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// Persistence failed while logging a mutation.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// Opaque catch-all for anything without a more specific service meaning.
    #[error(transparent)]
    Generic(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Inbound (driving) command port. Implemented by the domain
/// [`Service`](crate::domain::service::Service) and called by the inbound session, which
/// supplies the parsed [`Command`] and renders the returned [`CommandOutcome`] as a RESP
/// reply.
pub trait CacheService: Clone + Send + Sync + 'static {
    /// Run a command against the cache only — no persistence. Used on the replay path,
    /// where re-logging would duplicate the log.
    fn execute(&self, command: Command) -> Result<CommandOutcome, ServiceError>;
    /// Run a command and, if it mutates state, append it to the write-ahead log. The live
    /// client path.
    fn execute_logged(&self, command: Command) -> Result<CommandOutcome, ServiceError>;
}
