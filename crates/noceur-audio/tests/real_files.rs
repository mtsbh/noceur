//! Checking the quality analysis against a real library.
//!
//! Ignored by default: it needs somebody's music, and the point of it is to be
//! run by hand against files whose provenance is known.
use noceur_audio::quality;

#[test]
#[ignore]
fn report_on_a_folder() {
    let root = std::env::var("NOCEUR_SCAN").unwrap_or_else(|_| "E:/music".into());
    let limit: usize = std::env::var("NOCEUR_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let quiet = std::env::var("NOCEUR_QUIET").is_ok();
    let mut seen = 0;
    let mut tally = std::collections::BTreeMap::<&str, usize>::new();

    for entry in walk(std::path::Path::new(&root), 0) {
        let ext = entry
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !matches!(
            ext.as_str(),
            "mp3" | "flac" | "wav" | "aiff" | "aif" | "m4a"
        ) {
            continue;
        }
        let lossless = matches!(ext.as_str(), "flac" | "wav" | "aiff" | "aif");
        let claimed = claimed_kbps(&entry);
        let claim = if lossless {
            quality::Claim::Lossless
        } else {
            quality::Claim::Lossy(claimed)
        };
        let started = std::time::Instant::now();
        match quality::analyse(&entry) {
            Ok(s) => {
                let r = quality::judge(&s, claim);
                *tally.entry(r.verdict.as_str()).or_default() += 1;
                if !quiet || r.verdict != quality::Verdict::Genuine {
                    println!(
                        "{:11} {:>5}k {:>6.1}kHz {:>4}ms  {}  [{}]",
                        r.verdict.as_str(),
                        claimed
                            .map(|k| k.to_string())
                            .unwrap_or_else(|| "loss".into()),
                        r.cutoff_hz / 1000.0,
                        started.elapsed().as_millis(),
                        entry.file_name().unwrap().to_string_lossy(),
                        r.reason
                    );
                }
            }
            Err(e) => println!("ERR {} — {e}", entry.display()),
        }
        seen += 1;
        if seen >= limit {
            break;
        }
    }
    println!("\n{seen} files");
}

/// What the container says its bitrate is, for lossy formats only.
fn claimed_kbps(path: &std::path::Path) -> Option<u32> {
    use lofty::file::AudioFile;
    let tagged = lofty::probe::Probe::open(path).ok()?.read().ok()?;
    let p = tagged.properties();
    p.audio_bitrate()
}

fn walk(dir: &std::path::Path, depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if depth > 3 {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p, depth + 1));
        } else {
            out.push(p);
        }
    }
    out
}
