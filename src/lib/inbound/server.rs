//! TCP server and lifecycle. Owns the accept loop, the shared [`Cache`], and the
//! background threads (persistence ticker, TTL sweeper, stdin shutdown listener).
//!
//! ## Thread topology
//!
//! - **main thread** — accept loop, spawns per-connection [`Session`] threads.
//! - **persistence thread** — calls `cache.persist` every 10s, plus a final persist on
//!   shutdown.
//! - **sweeper thread** — calls `cache.remove_expired` every 10s for active TTL eviction.
//! - **shutdown thread** — blocks on stdin; EOF / `quit` / `exit` flips the shared flag.
//! - **session threads** — one per accepted connection, joined on shutdown.
//!
//! All long-running threads hold an `Arc<AtomicBool>` shutdown flag and check it on a
//! 100ms-tick budget so shutdown latency is bounded. `JoinHandle`s are collected and
//! pruned via `is_finished()` while the server runs, and joined fully on exit so no
//! work is silently dropped.
//!
//! The listener is set non-blocking so the accept loop polls the shutdown flag without
//! getting stuck inside `accept()`. A 50ms sleep on `WouldBlock` keeps the loop from
//! burning a CPU when no clients are connecting.

use crate::{domain::cache::Cache, inbound::session::Session};
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{JoinHandle, spawn},
    time::Duration,
};

/// Address the server binds for client connections.
const BIND_ADDRESS: &str = "127.0.0.1:3000";
/// On-disk path for the cache snapshot. Lives here (not in `cache.rs`) so the cache
/// module stays I/O-policy-agnostic — `Cache::init` / `Cache::persist` both take an
/// `impl Into<PathBuf>`.
const CACHE_PATH: &str = "cache";

/// Zero-sized handle exposing [`Server::run`]. The server itself is just a function;
/// the struct exists so callers have a stable `Server::run` entry point.
pub struct Server;
impl Server {
    /// Start the server. Blocks until shutdown is signaled (stdin EOF / `quit` / `exit`),
    /// then joins every spawned thread before returning.
    ///
    /// Returns `Err` only on a fatal setup error (bind failure, snapshot load failure).
    /// Per-connection or per-thread errors are logged but do not bubble out.
    pub fn run() -> anyhow::Result<()> {
        let listener = TcpListener::bind(BIND_ADDRESS)?;
        listener.set_nonblocking(true)?;
        println!("listening on {}", BIND_ADDRESS);

        let cache = Cache::init(CACHE_PATH)?;
        cache.remove_expired()?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let mut id = 0;

        let mut handles = Vec::<JoinHandle<()>>::new();

        // persistence
        let cache_clone = cache.clone();
        let persist = move || match cache_clone.persist(CACHE_PATH) {
            Ok(_) => println!("cache persisted"),
            Err(e) => eprintln!("failed to persist cache: {}", e),
        };
        let shutdown_clone = shutdown.clone();
        handles.push(spawn(move || {
            loop {
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if shutdown_clone.load(Ordering::Relaxed) {
                        persist();
                        return;
                    }
                }
                persist();
            }
        }));

        // shutdown
        let shutdown_clone = shutdown.clone();
        handles.push(spawn(move || {
            let mut s = String::new();
            loop {
                s.clear();
                match std::io::stdin().read_line(&mut s) {
                    Ok(0) => {
                        shutdown_clone.store(true, Ordering::Relaxed);
                        break;
                    }
                    Ok(_) if matches!(s.trim().to_lowercase().as_str(), "quit" | "exit") => {
                        shutdown_clone.store(true, Ordering::Relaxed);
                        break;
                    }
                    _ => (),
                }
            }
        }));

        // ttl
        let cache_clone = cache.clone();
        let remove_expired = move || match cache_clone.remove_expired() {
            Ok(expired) => println!("{} expired keys removed", expired),
            Err(e) => eprintln!("failed to remove expired keys: {}", e),
        };
        let shutdown_clone = shutdown.clone();
        handles.push(spawn(move || {
            loop {
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if shutdown_clone.load(Ordering::Relaxed) {
                        remove_expired();
                        return;
                    }
                }
                remove_expired();
            }
        }));

        // main
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            handles.retain(|t| !t.is_finished());
            match listener.accept() {
                Ok((writer_stream, _)) => {
                    let cache_clone = cache.clone();
                    let shutdown_clone = shutdown.clone();
                    id += 1;
                    println!("client number {} connected", id);
                    handles.push(spawn(move || {
                        let reader_stream = match writer_stream.try_clone() {
                            Ok(stream) => stream,
                            Err(e) => {
                                eprintln!("failed to clone stream: {}", e);
                                return;
                            }
                        };
                        if let Err(e) =
                            reader_stream.set_read_timeout(Some(Duration::from_millis(500)))
                        {
                            eprintln!("failed to set stream read timeout: {}", e);
                        }
                        let mut session =
                            Session::new(id, reader_stream, writer_stream, cache_clone);
                        match session.repl(shutdown_clone) {
                            Ok(_) => (),
                            Err(e) => eprintln!("failed to repl: {}", e),
                        }
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                Err(e) => eprintln!("failed to accept connection: {}", e),
            }
        }
        for h in handles {
            let _ = h.join();
        }
        Ok(())
    }
}
