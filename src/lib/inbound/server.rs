use crate::{domain::cache::Cache, inbound::session::Session};
use std::{
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{JoinHandle, spawn},
};

const BIND_ADDRESS: &str = "127.0.0.1:3000";

pub struct Server;
impl Server {
    pub fn run() -> anyhow::Result<()> {
        let listener = TcpListener::bind(BIND_ADDRESS)?;
        listener.set_nonblocking(true)?;
        println!("listening on {}", BIND_ADDRESS);

        let cache = Cache::init()?;
        cache.remove_expired()?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let mut id = 0;

        let mut handles = Vec::<JoinHandle<()>>::new();

        // persistence
        let cache_clone = cache.clone();
        let persist = move || match cache_clone.persist() {
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
