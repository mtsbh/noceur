//! Prints the key of each file given, and how long it took.
//!
//! `cargo run --release -p noceur-audio --example key -- <file>...`
//!
//! With `NOCEUR_DUMP=<dir>`, also writes each file's spectrogram there as raw
//! little-endian `f32` (105 rows, one per bin), for comparing against librosa.
fn main() {
    let dump = std::env::var("NOCEUR_DUMP").ok();
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let started = std::time::Instant::now();
        match noceur_audio::key::detect(path) {
            Ok(Some(e)) => println!(
                "{:>4}  conf {:.2}  {:>5} ms  {arg}",
                e.camelot,
                e.confidence,
                started.elapsed().as_millis()
            ),
            Ok(None) => println!("no key  {arg}"),
            Err(e) => println!("error: {e}  {arg}"),
        }
        if let Some(dir) = &dump {
            let (samples, rate) = decode(path);
            let (spec, frames) = noceur_audio::key::cqt(&samples, rate);
            let name = path.file_stem().unwrap().to_string_lossy();
            let bytes: Vec<u8> = spec.iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(format!("{dir}/{name}.{frames}.f32"), bytes).unwrap();
        }
    }
}

/// Mono samples and rate, the way `key::detect` reads them.
fn decode(path: &std::path::Path) -> (Vec<f32>, u32) {
    let file = std::fs::File::open(path).unwrap();
    let len = file.metadata().unwrap().len();
    let mut builder = rodio::Decoder::builder()
        .with_data(std::io::BufReader::new(file))
        .with_byte_len(len);
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().unwrap();
    let channels = rodio::Source::channels(&decoder).get() as usize;
    let rate = rodio::Source::sample_rate(&decoder).get();
    let all: Vec<f32> = decoder.collect();
    let mono = all
        .chunks_exact(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect();
    (mono, rate)
}
