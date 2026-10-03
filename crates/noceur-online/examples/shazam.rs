//! Asks Shazam what each file is, for checking the signature against the
//! real service.
//!
//! ```text
//! cargo run --release -p noceur-online --example shazam -- "track.flac" …
//! ```

fn main() {
    for path in std::env::args().skip(1) {
        let started = std::time::Instant::now();
        let sig = match noceur_audio::shazam::signature(std::path::Path::new(&path)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        let made = started.elapsed();
        match noceur_online::shazam::identify(&sig.uri, sig.sample_ms) {
            Ok(Some(m)) => println!(
                "{path}\n  -> {} - {} | {} | {} | {} | {}  ({made:?})",
                m.artist(),
                m.title,
                m.album.unwrap_or_default(),
                m.label.unwrap_or_default(),
                m.release_date.unwrap_or_default(),
                m.isrc.unwrap_or_default(),
            ),
            Ok(None) => println!("{path}\n  -> not recognised  ({made:?})"),
            Err(e) => println!("{path}\n  -> {e}"),
        }
    }
}
