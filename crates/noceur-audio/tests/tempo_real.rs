//! Checking tempo detection against the BPMs a real library is tagged with.
//!
//! Ignored by default: it needs somebody's music. Most DJ libraries have been
//! through Mixed In Key or a DJ program, so their BPM tags are a reference
//! worth measuring against before an estimate is offered to anyone.
//!
//! `NOCEUR_SCAN` is the folder (default `E:/music`), `NOCEUR_LIMIT` how many
//! files to sample from across it (default 200), `NOCEUR_MIN` / `NOCEUR_MAX`
//! the range, and `NOCEUR_VERBOSE` prints every file rather than only misses.
use noceur_audio::tempo::{self, Range};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[test]
#[ignore]
fn compare_with_tagged_bpm() {
    let root = std::env::var("NOCEUR_SCAN").unwrap_or_else(|_| "E:/music".into());
    let env = |k: &str, d: f64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let limit = env("NOCEUR_LIMIT", 200.0) as usize;
    let range = Range {
        min: env("NOCEUR_MIN", Range::default().min),
        max: env("NOCEUR_MAX", Range::default().max),
    };
    let verbose = std::env::var("NOCEUR_VERBOSE").is_ok();

    // Tagged files only, spread evenly across the whole library rather than
    // the first few hundred of one folder.
    let mut all: Vec<PathBuf> = walk(Path::new(&root), 0)
        .into_iter()
        .filter(|p| {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            matches!(
                ext.as_str(),
                "mp3" | "flac" | "wav" | "aiff" | "aif" | "m4a"
            )
        })
        .collect();
    all.sort();
    let step = (all.len() / (limit * 2)).max(1);
    let candidates: Vec<(PathBuf, f64)> = all
        .into_iter()
        .step_by(step)
        .filter_map(|p| {
            let bpm = noceur_tags::read(&p).ok()?.bpm? as f64;
            (bpm > 0.0).then_some((p, bpm))
        })
        .take(limit)
        .collect();

    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        // A few at a time: past that, a library on a spinning disk spends its
        // time seeking between files rather than reading any of them.
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((path, tagged)) = candidates.get(i) else {
                    break;
                };
                let t = std::time::Instant::now();
                let got = tempo::detect(path, range);
                let ms = t.elapsed().as_millis();
                results
                    .lock()
                    .unwrap()
                    .push((path.clone(), *tagged, got, ms));
            });
        }
    });

    let mut tally = std::collections::BTreeMap::<&str, usize>::new();
    let mut total_ms = 0;
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, tagged, got, ms) in &results {
        total_ms += ms;
        let name = path.file_name().unwrap().to_string_lossy();
        let (verdict, bpm, conf) = match got {
            Ok(Some(e)) => (classify(e.bpm, *tagged), e.bpm, e.confidence),
            Ok(None) => ("none", 0.0, 0.0),
            Err(e) => {
                println!("ERR {name}: {e}");
                ("error", 0.0, 0.0)
            }
        };
        *tally.entry(verdict).or_default() += 1;
        if verbose || !verdict.starts_with("exact") {
            println!(
                "{verdict:8} tag {tagged:>6.1}  got {bpm:>6.2}  conf {conf:.2}  {ms:>5}ms  {name}"
            );
        }
    }

    let n = results.len().max(1);
    println!(
        "\n{} files, range {}–{}",
        results.len(),
        range.min,
        range.max
    );
    for (k, v) in &tally {
        println!("{k:8} {v:>4}  {:>5.1}%", *v as f64 * 100.0 / n as f64);
    }
    println!(
        "{} ms a file on average, {:.1} s wall",
        total_ms / n as u128,
        started.elapsed().as_secs_f64()
    );
}

/// How an estimate relates to the tag.
///
/// `exact` means it rounds to the same whole BPM, or is within half a BPM of a
/// decimal tag; `close` within 1.5, which is still a mix-by-ear difference.
/// `half` and `double` are octave errors, `third` a 2:3 or 3:2 confusion.
fn classify(got: f64, tagged: f64) -> &'static str {
    let near = |a: f64, b: f64, tol: f64| (a - b).abs() <= tol;
    if near(got, tagged, 0.5) || got.round() == tagged.round() {
        "exact"
    } else if near(got, tagged, 1.5) {
        "close"
    } else if near(got * 2.0, tagged, 1.5) {
        "half"
    } else if near(got / 2.0, tagged, 1.0) {
        "double"
    } else if near(got * 1.5, tagged, 1.5) || near(got / 1.5, tagged, 1.5) {
        "third"
    } else {
        "wrong"
    }
}

fn walk(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if depth > 6 {
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
