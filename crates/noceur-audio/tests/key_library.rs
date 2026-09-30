//! Key detection against the key tags a real library carries.
//!
//! Ignored by default: it needs somebody's music. The tags are not ground
//! truth — they came from whatever tagged the files — but no model here was
//! trained on them, which makes them a fair way to compare models with each
//! other. `NOCEUR_SCAN` is the folder, `NOCEUR_LIMIT` how many tagged files to
//! sample (default 150), `NOCEUR_KEY_MODELS` extra weight files to measure
//! alongside the bundled one, separated by `;`, and `NOCEUR_MIK` keeps only
//! files Mixed In Key analysed.
use noceur_audio::key::{self, Model};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[test]
#[ignore]
fn agreement_with_tagged_keys() {
    let root = std::env::var("NOCEUR_SCAN").unwrap_or_else(|_| "E:/music".into());
    let limit: usize = std::env::var("NOCEUR_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    // Only files Mixed In Key itself analysed: it writes a BPM and a comment
    // that starts with the key and the energy, `8A - 7`. A key tag without
    // those came from somewhere else.
    let mik_only = std::env::var("NOCEUR_MIK").is_ok();
    let mut models: Vec<(String, Model)> = Vec::new();
    for p in std::env::var("NOCEUR_KEY_MODELS")
        .unwrap_or_default()
        .split(';')
    {
        if !p.is_empty() {
            let m = Model::parse(&std::fs::read(p).unwrap()).expect("weights");
            models.push((
                Path::new(p).file_stem().unwrap().to_string_lossy().into(),
                m,
            ));
        }
    }

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
    // Every eligible file first, then an even spread of them: sampling before
    // filtering leaves only a handful when few files qualify.
    let eligible: Vec<(PathBuf, usize)> = all
        .into_iter()
        .filter_map(|p| {
            let meta = noceur_tags::read(&p).ok()?;
            let c = class(&meta.key.as_ref()?.format(noceur_tags::KeyNotation::Camelot))?;
            (!mik_only || analysed_by_mik(&meta, c)).then_some((p, c))
        })
        .collect();
    let step = (eligible.len() / limit).max(1);
    let tracks: Vec<(PathBuf, usize)> = eligible.into_iter().step_by(step).take(limit).collect();

    let names: Vec<String> = std::iter::once("bundled".to_string())
        .chain(models.iter().map(|(n, _)| n.clone()))
        .collect();
    let next = AtomicUsize::new(0);
    let tallies = Mutex::new(vec![[0usize; 5]; names.len()]);
    std::thread::scope(|s| {
        for _ in 0..3 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((path, tag)) = tracks.get(i) else {
                    break;
                };
                let Some((samples, rate, ch)) = decode(path) else {
                    continue;
                };
                let mut row = Vec::new();
                let all = std::iter::once(Model::bundled()).chain(models.iter().map(|(_, m)| m));
                for (j, m) in all.enumerate() {
                    let got =
                        key::estimate_with(m, &samples, rate, ch).and_then(|e| class(&e.camelot));
                    let cat = got.map_or(4, |g| mirex(g, *tag));
                    tallies.lock().unwrap()[j][cat] += 1;
                    row.push(got.map_or("—".into(), camelot));
                }
                println!(
                    "tag {:>3}  {}  {}",
                    camelot(*tag),
                    row.join(" "),
                    path.file_name().unwrap().to_string_lossy()
                );
            });
        }
    });

    println!("\n{} files", tracks.len());
    for (name, t) in names.iter().zip(tallies.into_inner().unwrap()) {
        let n = t.iter().sum::<usize>().max(1) as f64;
        let pct = |c: usize| 100.0 * t[c] as f64 / n;
        println!(
            "{name:10} weighted {:5.1}  same {:5.1}  fifth {:4.1}  relative {:4.1}  parallel {:4.1}  other {:4.1}",
            pct(0) + 0.5 * pct(1) + 0.3 * pct(2) + 0.2 * pct(3),
            pct(0), pct(1), pct(2), pct(3), pct(4)
        );
    }
}

/// Whether a file carries Mixed In Key's marks for the key it is tagged with.
fn analysed_by_mik(meta: &noceur_tags::TrackMeta, class: usize) -> bool {
    let key = camelot(class);
    let comment = meta.comment.as_deref().unwrap_or("");
    meta.bpm.is_some_and(|b| b > 0.0)
        && comment
            .strip_prefix(&key)
            .and_then(|rest| rest.strip_prefix(" - "))
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

fn class(c: &str) -> Option<usize> {
    let (num, letter) = c.split_at(c.len().checked_sub(1)?);
    let n: usize = num.parse().ok()?;
    (1..=12)
        .contains(&n)
        .then(|| n - 1 + if letter == "B" { 12 } else { 0 })
}

fn camelot(c: usize) -> String {
    format!("{}{}", c % 12 + 1, if c < 12 { 'A' } else { 'B' })
}

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

fn decode(path: &Path) -> Option<(Vec<f32>, u32, usize)> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut b = rodio::Decoder::builder()
        .with_data(std::io::BufReader::new(file))
        .with_byte_len(len);
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        b = b.with_hint(ext);
    }
    let d = b.build().ok()?;
    let ch = rodio::Source::channels(&d).get() as usize;
    let rate = rodio::Source::sample_rate(&d).get();
    Some((d.take(rate as usize * ch * 600).collect(), rate, ch))
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
