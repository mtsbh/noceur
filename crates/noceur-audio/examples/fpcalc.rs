//! Prints a file's fingerprint the way `fpcalc` does, for checking against it
//! or pasting into an AcoustID lookup by hand.
//!
//! ```text
//! cargo run --release -p noceur-audio --example fpcalc -- "track.flac"
//! ```

fn main() {
    for path in std::env::args().skip(1) {
        let started = std::time::Instant::now();
        match noceur_audio::fingerprint::fingerprint(std::path::Path::new(&path)) {
            Ok(fp) => {
                println!("FILE={path}");
                println!("DURATION={}", fp.duration_secs.unwrap_or(0));
                println!("FINGERPRINT={}", fp.encoded);
                eprintln!("took {:?}", started.elapsed());
            }
            Err(e) => eprintln!("{path}: {e}"),
        }
    }
}
