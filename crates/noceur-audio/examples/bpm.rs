//! Prints the tempo of each file given, and how long it took.
//!
//! `cargo run --release -p noceur-audio --example bpm -- <file>...`
fn main() {
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let started = std::time::Instant::now();
        match noceur_audio::tempo::detect(path, Default::default()) {
            Ok(Some(e)) => println!(
                "{:>7.2} BPM  conf {:.2}  {:>5} ms  {arg}",
                e.bpm,
                e.confidence,
                started.elapsed().as_millis()
            ),
            Ok(None) => println!("no tempo  {arg}"),
            Err(e) => println!("error: {e}  {arg}"),
        }
    }
}
