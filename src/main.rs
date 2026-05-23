use diprotodon::inbound::server::Server;

fn main() {
    if let Err(e) = Server::run() {
        eprintln!("Failed to run: {}", e);
    }
}
