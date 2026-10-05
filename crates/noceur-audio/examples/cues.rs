//! Scores cue detection against cues already placed (by Mixed In Key, read out
//! of a rekordbox.xml into a TSV: path, bpm, grid start, its beat number, cue
//! times comma-separated).
//!
//! `cargo run --release -p noceur-audio --example cues -- <truth.tsv> [tracks] [threads]`
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Truth {
    path: String,
    bpm: f64,
    downbeat: f64,
    cues: Vec<f64>,
    energy: Vec<i32>,
}

#[derive(Default)]
struct Tally {
    tracks: usize,
    failed: usize,
    bpm_ok: usize,
    phase_ms: Vec<f64>,
    bar_ok: usize,
    bar_off: [usize; 4],
    disagree_ours: usize,
    disagree_theirs: usize,
    disagree_cues: usize,
    theirs: usize,
    ours: usize,
    hits: usize,
    hits_loose: usize,
    hits_bar: usize,
    sections: std::collections::BTreeMap<&'static str, usize>,
    /// Our label against Mixed In Key's energy there, less the track's median.
    labels: std::collections::BTreeMap<(&'static str, i32), usize>,
    hits_2bar: usize,
}

/// The track's cues, from a cached spectrogram when `CUES_CACHE` names a
/// folder: decoding is nearly all the time, and tuning needs many runs.
fn analysed(path: &str) -> Result<Option<noceur_audio::cues::Cues>, String> {
    use noceur_audio::cues;
    let Ok(dir) = std::env::var("CUES_CACHE") else {
        return cues::detect(std::path::Path::new(path)).map_err(|e| e.to_string());
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(path, &mut h);
    let file = std::path::Path::new(&dir).join(format!("{:x}.spec", std::hash::Hasher::finish(&h)));
    if let Ok(bytes) = std::fs::read(&file) {
        let rate = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let spec: Vec<f32> = bytes[4..]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 100.0)
            .collect();
        return Ok(cues::from_spectrogram(&spec, rate));
    }
    let d = noceur_audio::decode(std::path::Path::new(path), 1200).map_err(|e| e.to_string())?;
    let (spec, rate) = cues::spectrogram(&d.samples, d.rate, d.channels);
    let mut bytes = rate.to_le_bytes().to_vec();
    for v in &spec {
        bytes.extend_from_slice(
            &((v * 100.0).round().clamp(-32768.0, 32767.0) as i16).to_le_bytes(),
        );
    }
    let _ = std::fs::write(&file, bytes);
    Ok(cues::from_spectrogram(&spec, rate))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let text = std::fs::read_to_string(&args[0]).unwrap();
    let all: Vec<Truth> = text
        .lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            let bpm: f64 = c[1].parse().ok()?;
            let t0: f64 = c[2].parse().ok()?;
            let b: f64 = c[3].parse().ok()?;
            Some(Truth {
                path: c[0].to_string(),
                bpm,
                downbeat: t0 - (b - 1.0) * 60.0 / bpm,
                cues: c[4].split(',').filter_map(|v| v.parse().ok()).collect(),
                energy: c
                    .get(5)
                    .map(|n| {
                        n.split('|')
                            .map(|e| e.trim_start_matches("Energy ").parse().unwrap_or(0))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    let want: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(100);
    let threads: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(8);
    let step = (all.len() / want).max(1);
    let picked: Vec<&Truth> = all.iter().step_by(step).take(want).collect();

    let next = AtomicUsize::new(0);
    let tally = Mutex::new(Tally::default());
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(t) = picked.get(i) else { break };
                let got = analysed(&t.path);
                let mut y = tally.lock().unwrap();
                y.tracks += 1;
                let Ok(Some(c)) = got else {
                    y.failed += 1;
                    continue;
                };
                let beat = 60.0 / t.bpm;
                if (c.grid.bpm - t.bpm).abs() < 0.1 {
                    y.bpm_ok += 1;
                    let d = (c.grid.first - t.downbeat) / beat;
                    let frac = d - d.round();
                    y.phase_ms.push(frac * beat * 1000.0);
                    y.bar_off[(d.round() as i64).rem_euclid(4) as usize] += 1;
                    if (d.round() as i64).rem_euclid(4) != 0 {
                        for &x in &t.cues[1..] {
                            let on = |origin: f64| {
                                let b = (x - origin) / beat;
                                (b - b.round()).abs() < 0.25
                                    && (b.round() as i64).rem_euclid(4) == 0
                            };
                            y.disagree_cues += 1;
                            if on(c.grid.first) {
                                y.disagree_ours += 1;
                            }
                            if on(t.downbeat) {
                                y.disagree_theirs += 1;
                            }
                        }
                    }
                    if (d.round() as i64).rem_euclid(4) == 0 {
                        y.bar_ok += 1;
                    }
                }
                for q in &c.cues {
                    *y.sections.entry(q.section.name()).or_default() += 1;
                }
                if t.energy.len() == t.cues.len() && !t.energy.is_empty() {
                    let mut e = t.energy.clone();
                    e.sort();
                    let median = e[e.len() / 2];
                    for q in c.cues.iter().skip(1) {
                        if let Some(i) = t
                            .cues
                            .iter()
                            .position(|x| (x - q.at).abs() < 60.0 / t.bpm * 4.5)
                        {
                            let d = (t.energy[i] - median).clamp(-3, 2);
                            *y.labels.entry((q.section.name(), d)).or_default() += 1;
                        }
                    }
                }
                let theirs = &t.cues[1..];
                let ours: Vec<f64> = c.cues.iter().skip(1).map(|c| c.at).collect();
                y.theirs += theirs.len();
                y.ours += ours.len();
                for &x in theirs {
                    if ours.iter().any(|o| (o - x).abs() < beat * 0.5) {
                        y.hits += 1;
                    }
                    if ours.iter().any(|o| (o - x).abs() < beat * 4.5) {
                        y.hits_bar += 1;
                    }
                    if ours.iter().any(|o| (o - x).abs() < beat * 8.5) {
                        y.hits_2bar += 1;
                    }
                    if ours.iter().any(|o| (o - x).abs() < beat * 16.5) {
                        y.hits_loose += 1;
                    }
                }
                if std::env::var("CUES_VERBOSE").is_ok() {
                    let list = |v: &[f64]| {
                        v.iter()
                            .map(|x| format!("{:.0}", x / beat / 4.0))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    println!(
                        "{:.2}/{:.2} bars theirs [{}] ours [{}]  {}",
                        c.grid.bpm,
                        t.bpm,
                        list(theirs),
                        list(&ours),
                        t.path
                    );
                }
            });
        }
    });
    let y = tally.into_inner().unwrap();
    let mut ms = y.phase_ms.clone();
    ms.sort_by(f64::total_cmp);
    let pct = |p: f64| ms.get(((ms.len() as f64 * p) as usize).min(ms.len().saturating_sub(1)));
    let within = |lim: f64| ms.iter().filter(|m| m.abs() < lim).count();
    println!(
        "{} tracks in {:.0}s, {} failed",
        y.tracks,
        started.elapsed().as_secs_f64(),
        y.failed
    );
    println!("bpm within 0.1: {}", y.bpm_ok);
    println!(
        "phase vs theirs (ms): p10 {:?} median {:?} p90 {:?}; within 20ms {} / 50ms {}",
        pct(0.1),
        pct(0.5),
        pct(0.9),
        within(20.0),
        within(50.0)
    );
    println!(
        "downbeat right: {} of {} (off by 0/1/2/3 beats: {:?})",
        y.bar_ok, y.bpm_ok, y.bar_off
    );
    let (p, r) = (
        y.hits as f64 / y.ours.max(1) as f64,
        y.hits as f64 / y.theirs.max(1) as f64,
    );
    println!(
        "cues: theirs {} ours {} hits {} — precision {:.2} recall {:.2} F1 {:.2}; within 4 bars {:.2}",
        y.theirs,
        y.ours,
        y.hits,
        p,
        r,
        2.0 * p * r / (p + r).max(1e-9),
        y.hits_loose as f64 / y.theirs.max(1) as f64
    );
    println!(
        "where downbeats differ, their cues sit on our bar lines {} / theirs {} of {}",
        y.disagree_ours, y.disagree_theirs, y.disagree_cues
    );
    println!("sections {:?}", y.sections);
    println!("ours vs their energy - median: {:?}", y.labels);
    println!(
        "recall within 1 bar {:.2}, 2 bars {:.2}",
        y.hits_bar as f64 / y.theirs.max(1) as f64,
        y.hits_2bar as f64 / y.theirs.max(1) as f64
    );
}
