fn main() {
    if let Err(error) = monlin::run(std::env::args()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
