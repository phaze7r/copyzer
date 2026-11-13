fn main() {
    if let Err(err) = copyzer::cli::run() {
        eprintln!("copyzer: {err:#}");
        std::process::exit(1);
    }
}
