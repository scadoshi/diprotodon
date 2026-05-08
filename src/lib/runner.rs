use crate::command::Command;
use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    thread::spawn,
};

const BIND_ADDRESS: &str = "127.0.0.1:3000";
const KILOBYTE_SIZE: usize = 1024;

fn handle_connection(id: u32, mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = vec![0; KILOBYTE_SIZE];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                println!("client {} disconnected", id);
                break;
            }
            Ok(n) => {
                println!("client {}: received {} bytes", id, n);
                let client_message = String::from_utf8_lossy(&buf[..n]).to_string();
                println!("client {}: {}", id, client_message);
                match Command::try_from(client_message.as_str()) {
                    Ok(command) => println!("parsed command: `{:?}`", command),
                    Err(e) => eprintln!("failed to parse command: {}", e),
                }
                buf.clear();
                buf.resize(KILOBYTE_SIZE, 0);
            }
            Err(e) => {
                eprintln!("error reading from client {}'s stream: {}", id, e);
                return Err(e);
            }
        }
    }
    Ok(())
}

pub struct Runner;
impl Runner {
    pub fn run() {
        let listener = TcpListener::bind(BIND_ADDRESS).expect("failed to bind to BIND_ADDRESS");
        println!("listening on {}", BIND_ADDRESS);
        let mut connection_count = 0;
        loop {
            if let Ok((stream, _)) = listener.accept() {
                connection_count += 1;
                println!("client number {} connected", connection_count);
                spawn(move || handle_connection(connection_count, stream));
            }
        }
    }
}
