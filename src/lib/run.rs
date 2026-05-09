use std::{net::TcpListener, thread::spawn};

use crate::{cache::Cache, session::Session};

const BIND_ADDRESS: &str = "127.0.0.1:3000";

pub struct Runner;
impl Runner {
    pub fn run() -> anyhow::Result<()> {
        let listener = TcpListener::bind(BIND_ADDRESS)?;
        println!("listening on {}", BIND_ADDRESS);
        let mut connection_count = 0;
        let cache = Cache::init()?;
        let persisting_cache = cache.clone();
        spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(10));
                match persisting_cache.persist() {
                    Ok(_) => println!("Cache persisted"),
                    Err(e) => eprintln!("Failed to persist cache: {}", e),
                }
            }
        });
        loop {
            if let Ok((stream, _)) = listener.accept() {
                connection_count += 1;
                println!("client number {} connected", connection_count);
                let cache_clone = cache.clone();
                spawn(
                    move || match Session::new(connection_count, stream, cache_clone) {
                        Ok(mut session) => match session.repl() {
                            Ok(_) => (),
                            Err(e) => eprintln!("Failed to repl: {}", e),
                        },
                        Err(e) => eprintln!("Failed to create new session: {}", e),
                    },
                );
            }
        }
    }
}
