use diprotodon::run::Runner;

fn main() {
    if let Err(e) = Runner::run() {
        eprintln!("Failed to run: {}", e);
    }
}
