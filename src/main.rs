fn main() {
    if let Err(error) = nxu_cpu::run(std::env::args()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
