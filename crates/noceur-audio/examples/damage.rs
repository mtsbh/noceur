//! Runs the damage check over files and prints what it finds.
//!
//! `cargo run --release -p noceur-audio --example damage -- <file or folder>...`
//!
//! Folders are walked. Every file is printed with its counts, so a tuning run
//! shows what the detector saw on clean files as well as on broken ones.

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

    let mut damaged = 0;
    for path in &files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        match noceur_audio::quality::analyse(path) {
            Ok(s) => {
                let d = &s.damage;
                let claimed = noceur_tags::read(path).ok().and_then(|m| m.bit_depth);
                let summary = d.summary(claimed);
                if !summary.is_empty() {
                    damaged += 1;
                }
                println!(
                    "{:<40} err {:>3} stated {:>7} got {:>7.1} drop {:>2} ({:>5.0} ms) clicks {:>3} bits {:>2}/{:<2} | {}",
                    truncate(&name, 40),
                    d.decode_errors,
                    d.stated_secs.map(|s| format!("{s:.1}")).unwrap_or("-".into()),
                    d.decoded_secs,
                    d.dropouts,
                    d.longest_dropout_ms,
                    d.clicks,
                    d.used_bits.map(|b| b.to_string()).unwrap_or("-".into()),
                    claimed.map(|b| b.to_string()).unwrap_or("-".into()),
                    summary,
                );
                if let Some(why) = &d.stopped {
                    println!("    stopped: {why}");
                }
            }
            Err(e) => {
                damaged += 1;
                println!("{:<40} UNREADABLE {e}", truncate(&name, 40));
            }
        }
    }
    println!("\n{damaged} of {} with something to report", files.len());
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
