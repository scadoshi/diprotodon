use std::{net::TcpListener, thread::spawn};

use crate::{domain::cache::Cache, inbound::session::Session};

const BIND_ADDRESS: &str = "127.0.0.1:3000";

pub struct Server;
impl Server {
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
                    Ok(_) => println!("cache persisted"),
                    Err(e) => eprintln!("failed to persist cache: {}", e),
                }
            }
        });
        loop {
            if let Ok((stream, _)) = listener.accept() {
                connection_count += 1;
                println!("client number {} connected", connection_count);
                let cache_clone = cache.clone();
                spawn(move || {
                    let stream_clone = match stream.try_clone() {
                        Ok(stream) => stream,
                        Err(e) => {
                            eprintln!("failed to clone stream: {}", e);
                            return;
                        }
                    };
                    let mut session =
                        Session::new(connection_count, stream_clone, stream, cache_clone);
                    match session.repl() {
                        Ok(_) => (),
                        Err(e) => eprintln!("failed to repl: {}", e),
                    }
                });
            }
        }
    }
}
