//! Times the key model on one file: `keytime <file> [weights.bin]`.
fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a file");
    let custom = args
        .next()
        .map(|p| noceur_audio::key::Model::parse(&std::fs::read(p).unwrap()).unwrap());
    let model = custom
        .as_ref()
        .unwrap_or_else(|| noceur_audio::key::Model::bundled());
    let t = std::time::Instant::now();
    let d = noceur_audio::decode(std::path::Path::new(&path), noceur_audio::ANALYSIS_SECS).unwrap();
    println!("decode {} ms", t.elapsed().as_millis());
    let t = std::time::Instant::now();
    let e = noceur_audio::key::estimate_with(model, &d.samples, d.rate, d.channels);
    println!("cqt + network {} ms  {:?}", t.elapsed().as_millis(), e);
}
