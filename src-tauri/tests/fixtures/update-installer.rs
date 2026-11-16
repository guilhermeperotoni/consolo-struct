//! Executável descartável dos testes Windows; nunca é empacotado no aplicativo.
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let directory = PathBuf::from(arguments.split_once("/D=").unwrap().1);
    std::fs::write(directory.join("started.json"), &arguments).unwrap();
    let start = Instant::now();
    while !directory.join("finish").exists() {
        if start.elapsed() > Duration::from_secs(15) {
            std::process::exit(3);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
