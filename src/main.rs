use std::{
    io::Read,
    net::{TcpListener, TcpStream},
    thread::spawn,
};

const BIND_ADDRESS: &str = "127.0.0.1:3000";

fn handle_connection(id: u32, mut stream: TcpStream) -> std::io::Result<()> {
    let mut buf = [0; 512];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                println!("client {} disconnected", id);
                break;
            }
            Ok(n) => {
                println!("client {}: received {} bytes", id, n);
                println!("client {}: {}", id, String::from_utf8_lossy(&buf));
            }
            Err(e) => {
                eprintln!("error reading from client {}'s stream: {}", id, e);
                return Err(e);
            }
        }
    }
    Ok(())
}

fn listen() {
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

fn main() {
    listen();
}
