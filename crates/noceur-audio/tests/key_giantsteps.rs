//! Key detection on the GiantSteps key test set, scored the MIREX way.
//!
//! Ignored by default: it needs the dataset's audio (604 two-minute Beatport
//! previews, `audio_dl.sh` in github.com/GiantSteps/giantsteps-key-dataset).
//! `NOCEUR_GIANTSTEPS` is the dataset folder; `NOCEUR_KEY_MODEL` optionally a
//! weight file to measure instead of the bundled one.
use noceur_audio::key::{self, Model};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[test]
#[ignore]
fn mirex_score_on_giantsteps() {
    let root = PathBuf::from(std::env::var("NOCEUR_GIANTSTEPS").expect("set NOCEUR_GIANTSTEPS"));
    let custom = std::env::var("NOCEUR_KEY_MODEL")
        .ok()
        .map(|p| Model::parse(&std::fs::read(p).unwrap()).expect("weights"));
    let model = custom.as_ref().unwrap_or_else(|| Model::bundled());

    let mut tracks = Vec::new();
    for e in std::fs::read_dir(root.join("annotations/key"))
        .unwrap()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        let id = name.split('.').next().unwrap().to_string();
        let text = std::fs::read_to_string(e.path()).unwrap();
        if let Some(truth) = class_of(text.trim()) {
            tracks.push((id, truth));
        }
    }
    tracks.sort();

    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((id, truth)) = tracks.get(i) else {
                    break;
                };
                let path = root.join(format!("audio/{id}.LOFI.mp3"));
                let (samples, rate, channels) = decode(&path);
                let got = key::estimate_with(model, &samples, rate, channels);
                let class = got.and_then(|e| class_of_camelot(&e.camelot));
                found.lock().unwrap().push((id.clone(), *truth, class));
            });
        }
    });

    let found = found.into_inner().unwrap();
    let mut tally = [0usize; 5];
    for (_, truth, got) in &found {
        tally[got.map_or(4, |g| mirex(g, *truth))] += 1;
    }
    let n = found.len() as f64;
    let pct = |c: usize| 100.0 * tally[c] as f64 / n;
    let weighted = pct(0) + 0.5 * pct(1) + 0.3 * pct(2) + 0.2 * pct(3);
    println!(
        "{} tracks  weighted {weighted:.2}  correct {:.2}  fifth {:.2}  relative {:.2}  parallel {:.2}  other {:.2}  ({:.0} s)",
        found.len(),
        pct(0),
        pct(1),
        pct(2),
        pct(3),
        pct(4),
        started.elapsed().as_secs_f64()
    );
    if let Ok(out) = std::env::var("NOCEUR_KEY_OUT") {
        let lines: Vec<String> = found
            .iter()
            .map(|(id, _, g)| format!("{id}\t{}", g.map_or(-1, |g| g as i64)))
            .collect();
        std::fs::write(out, lines.join("\n")).unwrap();
    }
}

/// MusicalKeyCNN's class for a key name: Camelot number − 1, +12 for major.
fn class_of(name: &str) -> Option<usize> {
    const MINOR: [&str; 12] = [
        "G#", "D#", "A#", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
    ];
    const MAJOR: [&str; 12] = [
        "B", "F#", "C#", "G#", "D#", "A#", "F", "C", "G", "D", "A", "E",
    ];
    let (tonic, mode) = name.split_once(' ')?;
    let tonic = match tonic {
        "Ab" => "G#",
        "Eb" => "D#",
        "Bb" => "A#",
        "Gb" => "F#",
        "Db" => "C#",
        t => t,
    };
    match mode {
        "minor" => MINOR.iter().position(|t| *t == tonic),
        "major" => MAJOR.iter().position(|t| *t == tonic).map(|i| i + 12),
        _ => None,
    }
}

fn class_of_camelot(c: &str) -> Option<usize> {
    let (num, letter) = c.split_at(c.len() - 1);
    let n: usize = num.parse().ok()?;
    Some(n - 1 + if letter == "B" { 12 } else { 0 })
}

/// 0 correct, 1 fifth, 2 relative, 3 parallel, 4 other — MusicalKeyCNN's eval.
fn mirex(p: usize, g: usize) -> usize {
    if p == g {
        return 0;
    }
    let (mut tp, mut tg, mut same) = (p as i64, g as i64, false);
    if g > 11 && p > 11 {
        same = true;
        tp -= 12;
        tg -= 12;
    } else if g < 12 && p < 12 {
        same = true;
    }
    let d = tp.min(tg) - tp.max(tg);
    if same && (d == -1 || d == -11) {
        1
    } else if d == -12 {
        2
    } else if d == -15 {
        3
    } else {
        4
    }
}

fn decode(path: &std::path::Path) -> (Vec<f32>, u32, usize) {
    let file = std::fs::File::open(path).unwrap();
    let len = file.metadata().unwrap().len();
    let decoder = rodio::Decoder::builder()
        .with_data(std::io::BufReader::new(file))
        .with_byte_len(len)
        .with_hint("mp3")
        .build()
        .unwrap();
    let channels = rodio::Source::channels(&decoder).get() as usize;
    let rate = rodio::Source::sample_rate(&decoder).get();
    (decoder.collect(), rate, channels)
}
