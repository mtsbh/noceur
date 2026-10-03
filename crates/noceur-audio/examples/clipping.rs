//! Runs the clipping check over files and prints where each one's flat tops sit.
//!
//! `cargo run --release -p noceur-audio --example clipping -- <file or folder>...`
//!
//! Folders are walked. Every file is printed, so a tuning run shows what the
//! check saw on clean files as well as on clipped ones.

use std::path::{Path, PathBuf};

const AUDIO: &[&str] = &["mp3", "flac", "wav", "aif", "aiff", "m4a", "ogg"];

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            walk(&e, out);
        }
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO.contains(&e.to_ascii_lowercase().as_str()))
    {
        out.push(path.to_path_buf());
    }
}

fn main() {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        walk(Path::new(&arg), &mut files);
    }

    let (mut full, mut lowered) = (0, 0);
    for path in &files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match noceur_audio::quality::analyse(path) {
            Ok(s) => {
                let c = s.clipping;
                let mark = if c.longest_run >= 8 && c.percent >= 0.01 {
                    if c.level_db < 0.0 {
                        lowered += 1;
                        "LOWERED"
                    } else {
                        full += 1;
                        "FULL"
                    }
                } else {
                    ""
                };
                println!(
                    "{:<50} {:>6.2}% run {:>4} at {:>6.2} dB peak {:>6.2} {mark}",
                    truncate(&name, 50),
                    c.percent,
                    c.longest_run,
                    c.level_db,
                    20.0 * s.loudness.sample_peak.max(1e-9).log10(),
                );
            }
            Err(e) => println!("{:<50} UNREADABLE {e}", truncate(&name, 50)),
        }
    }
    println!(
        "\n{full} clipped at full scale, {lowered} clipped and turned down, of {}",
        files.len()
    );
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
