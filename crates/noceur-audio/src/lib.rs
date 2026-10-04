//! Preview playback.
//!
//! Playback here is for auditioning while browsing, not for performance: one
//! track at a time, no gapless, no crossfade. The design spec pulled it into the
//! browser sub-project because a seekable waveform needs a player underneath it.

pub mod biquad;
pub mod damage;
pub mod fade;
pub mod fft;
pub mod fingerprint;
pub mod identity;
pub mod key;
pub mod loudness;
pub mod prefetch;
pub mod quality;
pub mod scope;
pub mod shazam;
pub mod source;
pub mod spectrogram;
pub mod tempo;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output device is available: {0}")]
    NoDevice(String),
    #[error("cannot open {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot decode {path}: {reason}")]
    Decode { path: String, reason: String },
    #[error("the audio thread has stopped")]
    Gone,
}

/// What the transport shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playback {
    /// The file being played, if any.
    pub path: Option<String>,
    pub playing: bool,
    pub position_ms: u64,
    /// `None` until the decoder reports one — some streams never do.
    pub duration_ms: Option<u64>,
    /// Set when the last command failed, so the UI can say why nothing happened.
    pub error: Option<String>,
}

enum Cmd {
    Load {
        path: PathBuf,
        play: bool,
    },
    Resume,
    Pause,
    Toggle,
    Stop,
    Seek(Duration),
    Volume(f32),
    /// Answered once every command sent before it has been carried out and
    /// its effect published; see [`Player::settle`].
    Sync(Sender<()>),
}

/// A handle to the audio thread.
///
/// The output device and its mixer are neither `Send` nor safely movable between
/// threads, so they stay on one thread that owns them for the life of the process
/// and takes commands over a channel. That also means a device that disappears —
/// a USB interface unplugged mid-set — is one thread's problem rather than a
/// poisoned lock in the middle of the UI.
pub struct Player {
    tx: Sender<Cmd>,
    state: Arc<Mutex<Playback>>,
    scope: Arc<scope::Scope>,
}

impl Player {
    /// Starts the audio thread.
    ///
    /// Never fails: a machine with no output device still browses, and the
    /// failure is reported through [`Playback::error`] when something is asked
    /// of it rather than preventing the app from starting.
    pub fn start() -> Player {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let state = Arc::new(Mutex::new(Playback::default()));
        let thread_state = Arc::clone(&state);
        let scope = Arc::new(scope::Scope::default());
        let thread_scope = Arc::clone(&scope);

        std::thread::Builder::new()
            .name("noceur-audio".into())
            .spawn(move || run(rx, thread_state, thread_scope))
            .expect("spawning the audio thread");

        Player { tx, state, scope }
    }

    /// The newest `frames` stereo frames sent to the output, and their rate.
    ///
    /// Read by the meters; see [`scope`] for why it is taken before the volume.
    pub fn scope(&self, frames: usize) -> (u32, Vec<[f32; 2]>) {
        self.scope.snapshot(frames)
    }

    /// A snapshot of what is playing.
    pub fn state(&self) -> Playback {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn load(&self, path: &Path, play: bool) -> Result<(), AudioError> {
        self.send(Cmd::Load {
            path: path.to_path_buf(),
            play,
        })
    }

    /// Blocks until a [`load`](Self::load) has actually taken effect.
    ///
    /// `load` hands the path to the audio thread and returns at once. Opening
    /// the file, building a decoder and reading its length happen over there,
    /// and until they have, the player still describes the *previous* track —
    /// so anything that loads and immediately reports its state describes the
    /// wrong one.
    ///
    /// That was a race rather than a constant: an MP3 opened fast enough to win
    /// it and a large FLAC did not, which is why a FLAC appeared to need playing
    /// twice before it had a waveform, a duration or a position.
    ///
    /// Returns the state as of giving up, so a file that is genuinely slow — a
    /// long track on a cold network share — reports what is known rather than
    /// blocking a command indefinitely.
    pub fn wait_until_loaded(&self, path: &Path, timeout: Duration) -> Playback {
        let wanted = path.display().to_string();
        let deadline = std::time::Instant::now() + timeout;

        loop {
            let now = self.state();
            // The error belongs to this load only if it arrived after the
            // previous track was cleared, which `open` does by leaving the path
            // alone and setting the error. Either way there is nothing further
            // to wait for.
            if now.path.as_deref() == Some(wanted.as_str()) || now.error.is_some() {
                return now;
            }
            if std::time::Instant::now() >= deadline {
                return now;
            }
            // Short enough to be invisible on a file that opens quickly, long
            // enough not to spin a core while one does not.
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    pub fn resume(&self) -> Result<(), AudioError> {
        self.send(Cmd::Resume)
    }

    pub fn pause(&self) -> Result<(), AudioError> {
        self.send(Cmd::Pause)
    }

    /// Play if paused, pause if playing — what the space bar does.
    pub fn toggle(&self) -> Result<(), AudioError> {
        self.send(Cmd::Toggle)
    }

    /// Blocks until the audio thread has caught up with every command sent so
    /// far, or `timeout` passes.
    ///
    /// Commands return as soon as they are queued, so a state read straight
    /// after one describes the player *before* it. For a resume that was fatal:
    /// the reply said "paused", the transport stopped polling because nothing
    /// was playing, and the bar and meters sat frozen over audible music.
    pub fn settle(&self, timeout: Duration) {
        let (ack, done) = mpsc::channel();
        if self.send(Cmd::Sync(ack)).is_ok() {
            // A dead thread drops the sender, which ends the wait at once.
            let _ = done.recv_timeout(timeout);
        }
    }

    pub fn stop(&self) -> Result<(), AudioError> {
        self.send(Cmd::Stop)
    }

    pub fn seek(&self, position: Duration) -> Result<(), AudioError> {
        self.send(Cmd::Seek(position))
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), AudioError> {
        self.send(Cmd::Volume(volume.clamp(0.0, 1.0)))
    }

    fn send(&self, cmd: Cmd) -> Result<(), AudioError> {
        self.tx.send(cmd).map_err(|_| AudioError::Gone)
    }
}

/// The audio thread's whole life.
fn run(rx: mpsc::Receiver<Cmd>, state: Arc<Mutex<Playback>>, scope: Arc<scope::Scope>) {
    let sink = match rodio::stream::DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => sink,
        Err(e) => {
            set(&state, |s| {
                s.error = Some(AudioError::NoDevice(e.to_string()).to_string())
            });
            // Drain commands so senders never block on a dead channel; every one
            // of them is a no-op without a device.
            while rx.recv().is_ok() {}
            return;
        }
    };

    let mut player: Option<rodio::Player> = None;
    // Ramps the current track down when it is let go; see `fade`.
    let mut fade_out = fade::FadeOut::default();
    let mut volume: f32 = 1.0;

    loop {
        // Wake regularly even with no commands, so the reported position keeps
        // up with the audio rather than only moving when something is clicked.
        let cmd = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(cmd) => Some(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };

        if let Some(cmd) = cmd {
            match cmd {
                Cmd::Load { path, play } => {
                    // Cleared before opening rather than after. Two reasons: the
                    // state must never go on describing the previous track while
                    // this one is being opened, and a leftover error from a file
                    // that would not play must not be read as this load's
                    // outcome by anything waiting for it.
                    set(&state, |s| {
                        s.path = None;
                        s.playing = false;
                        s.position_ms = 0;
                        s.duration_ms = None;
                        s.error = None;
                    });
                    scope.clear();

                    // The old track plays on while this one opens, then fades
                    // out under the new one's fade in rather than being cut.
                    match open(&path, sink.mixer(), volume, &scope) {
                        Ok((new_player, new_fade, duration)) => {
                            if !play {
                                new_player.pause();
                            }
                            if let Some(old) = player.take() {
                                let_go(old, &fade_out);
                            }
                            player = Some(new_player);
                            fade_out = new_fade;
                            set(&state, |s| {
                                s.path = Some(path.display().to_string());
                                s.playing = play;
                                s.position_ms = 0;
                                s.duration_ms = duration;
                                s.error = None;
                            });
                        }
                        Err(e) => {
                            if let Some(old) = player.take() {
                                let_go(old, &fade_out);
                            }
                            set(&state, |s| {
                                s.path = None;
                                s.playing = false;
                                s.position_ms = 0;
                                s.duration_ms = None;
                                s.error = Some(e.to_string());
                            });
                        }
                    }
                }
                Cmd::Resume => {
                    if let Some(p) = &player {
                        p.play();
                    }
                }
                Cmd::Pause => {
                    if let Some(p) = &player {
                        p.pause();
                    }
                }
                Cmd::Toggle => {
                    if let Some(p) = &player {
                        if p.is_paused() {
                            p.play();
                        } else {
                            p.pause();
                        }
                    }
                }
                Cmd::Stop => {
                    if let Some(old) = player.take() {
                        let_go(old, &fade_out);
                    }
                    scope.clear();
                    set(&state, |s| {
                        s.path = None;
                        s.playing = false;
                        s.position_ms = 0;
                        s.duration_ms = None;
                    });
                }
                Cmd::Seek(pos) => {
                    if let Some(p) = &player {
                        if let Err(e) = p.try_seek(pos) {
                            set(&state, |s| s.error = Some(format!("cannot seek: {e}")));
                        }
                    }
                }
                Cmd::Volume(v) => {
                    volume = v;
                    if let Some(p) = &player {
                        p.set_volume(v);
                    }
                }
                // Everything before it was handled on an earlier pass, and each
                // pass ends by publishing the state, so the answer is already
                // true when it is given.
                Cmd::Sync(ack) => {
                    let _ = ack.send(());
                }
            }
        }

        // Publish where the audio actually is, not where the last command left
        // it: pausing, seeking and reaching the end all move it.
        if let Some(p) = &player {
            let position = p.get_pos();
            let finished = p.empty();
            set(&state, |s| {
                s.position_ms = position.as_millis() as u64;
                s.playing = !p.is_paused() && !finished;
            });
        }
    }
}

/// Stops a track without a click: one still sounding ramps down and ends
/// itself on the mixer; a paused one is silent already and simply goes.
fn let_go(player: rodio::Player, fade_out: &fade::FadeOut) {
    if player.is_paused() || player.empty() {
        player.stop();
    } else {
        fade_out.start();
        player.detach();
    }
}

/// Opens a file and starts it on the mixer, reporting its duration if known.
fn open(
    path: &Path,
    mixer: &rodio::mixer::Mixer,
    volume: f32,
    scope: &Arc<scope::Scope>,
) -> Result<(rodio::Player, fade::FadeOut, Option<u64>), AudioError> {
    let full = noceur_extended(path);
    let opened = source::open(&full).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let byte_len = opened.byte_len;

    let mut builder = rodio::Decoder::builder()
        .with_data(opened.reader)
        .with_seekable(true);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    // The extension is a strong hint for containers whose first bytes are
    // ambiguous, and costs nothing when the sniffer would have got there anyway.
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }

    let decoder = builder.build().map_err(|e| AudioError::Decode {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let duration = rodio::Source::total_duration(&decoder).map(|d| d.as_millis() as u64);

    let player = rodio::Player::connect_new(mixer);
    player.set_volume(volume);
    // Decoded on its own thread, never inside the device callback; see
    // `prefetch` for the noise that caused on lossless files.
    let decoded = prefetch::Prefetch::new(decoder);
    let (faded, fade_out) = fade::Fade::new(scope::Tap::new(decoded, Arc::clone(scope)));
    player.append(faded);

    Ok((player, fade_out, duration))
}

/// Long-path form, so a deeply nested library still opens.
fn noceur_extended(path: &Path) -> PathBuf {
    noceur_fsmodel::path::extended(path)
}

/// The longest stretch tempo and key are measured over. A club track is six
/// to eight minutes; past ten there is nothing more to learn, only more to
/// decode.
pub const ANALYSIS_SECS: usize = 600;

/// Decoded audio for analysis: interleaved samples, rate and channel count.
pub struct Decoded {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: usize,
}

/// Decodes up to `max_secs` of a file, for the analyses that need the samples
/// in memory (tempo, key).
pub fn decode(path: &Path, max_secs: usize) -> Result<Decoded, AudioError> {
    let full = noceur_extended(path);
    let opened = source::open(&full).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let mut builder = rodio::Decoder::builder().with_data(opened.reader);
    if let Some(len) = opened.byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }
    let decoder = builder.build().map_err(|e| AudioError::Decode {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let channels = rodio::Source::channels(&decoder).get() as usize;
    let rate = rodio::Source::sample_rate(&decoder).get();
    let samples = decoder.take(rate as usize * channels * max_secs).collect();
    Ok(Decoded {
        samples,
        rate,
        channels,
    })
}

fn set(state: &Arc<Mutex<Playback>>, f: impl FnOnce(&mut Playback)) {
    if let Ok(mut s) = state.lock() {
        f(&mut s);
    }
}

/// Buckets one envelope is stored at.
///
/// Fixed rather than per-display: an envelope costs a full decode of the track,
/// so it is computed once at a resolution finer than any transport bar and
/// aggregated down when drawn. Two thousand buckets is four kilobytes per track
/// and more detail than a 1400px bar can show.
pub const ENVELOPE_BUCKETS: usize = 2000;

/// Bytes each bucket occupies: peak, then low, mid and high RMS.
pub const ENVELOPE_BANDS: usize = 4;

/// Index of each band within a bucket.
pub const BAND_PEAK: usize = 0;
pub const BAND_LOW: usize = 1;
pub const BAND_MID: usize = 2;
pub const BAND_HIGH: usize = 3;

/// Amplitude envelope of a whole track: `[peak, low, mid, high]` per bucket.
///
/// **Split by frequency, because that is what makes a DJ waveform readable.**
/// A bucket spans a fifth of a second, and in any fifth of a second of a limited
/// club master the loudest sample is essentially full scale — so a peak-only
/// waveform of most of a library draws as a solid rectangle with no intro, no
/// breakdown and no outro visible. Splitting the same bucket into bass, body and
/// air fixes that twice over: the three levels together say how loud the music
/// is, and their balance says what *kind* of moment it is. A breakdown loses its
/// kick and turns blue; a drop takes it back and turns red. That is the whole
/// reason every DJ program colours its waveform this way, and it is legible at a
/// glance in a way an amplitude outline never is.
///
/// Peak is kept alongside them because it is what makes transients legible, and
/// drawn as a halo behind the body it costs one byte per bucket.
///
/// Bytes rather than floats: a waveform is drawn a few tens of pixels tall, so
/// 256 levels is well past the point of visible difference, and it keeps the
/// cached blob to eight kilobytes a track.
pub fn envelope(path: &Path, buckets: usize) -> Result<Vec<u8>, AudioError> {
    let buckets = buckets.max(1);
    let full = noceur_extended(path);
    let opened = source::open(&full).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;
    let byte_len = opened.byte_len;

    let mut builder = rodio::Decoder::builder().with_data(opened.reader);
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        builder = builder.with_hint(ext);
    }

    let decoder = builder.build().map_err(|e| AudioError::Decode {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let channels = rodio::Source::channels(&decoder).get() as u64;
    let rate = rodio::Source::sample_rate(&decoder).get() as u64;
    let duration = rodio::Source::total_duration(&decoder);

    // Frames per bucket, from the duration the decoder reports. A stream that
    // turns out longer than advertised piles into the last bucket rather than
    // running off the end; one that ends early leaves the tail flat. Both are
    // better than refusing to draw anything.
    let total_frames = duration
        .map(|d| (d.as_secs_f64() * rate as f64) as u64)
        .filter(|f| *f > 0)
        .unwrap_or(rate * 60 * 10);
    let per_bucket = (total_frames / buckets as u64).max(1);

    let mut peaks = vec![0f32; buckets];
    // Sum of squares and a count per bucket, so each RMS is one division and a
    // square root at the end rather than a running average that drifts.
    let mut squares = vec![[0f64; 3]; buckets];
    let mut counts = vec![0u64; buckets];

    let mut split = biquad::Split::new(rate as f32);
    let mut frame: u64 = 0;
    let mut channel: u64 = 0;
    let mut loudest: f32 = 0.0;
    let mut mono: f32 = 0.0;

    for sample in decoder {
        // Channels interleave; one frame is one sample from each. Peak takes the
        // loudest of them rather than one arbitrary side. The filterbank gets
        // their mean, because it is a recursive filter: feeding it left and right
        // alternately would halve its effective sample rate and move every
        // crossover an octave down.
        loudest = loudest.max(sample.abs());
        mono += sample;
        channel += 1;
        if channel < channels {
            continue;
        }
        channel = 0;
        mono /= channels as f32;

        let bands = split.step(mono);
        let bucket = ((frame / per_bucket) as usize).min(buckets - 1);
        if loudest > peaks[bucket] {
            peaks[bucket] = loudest;
        }
        for b in 0..3 {
            squares[bucket][b] += (bands[b] as f64) * (bands[b] as f64);
        }
        counts[bucket] += 1;

        loudest = 0.0;
        mono = 0.0;
        frame += 1;
    }

    let mut out = vec![0u8; buckets * ENVELOPE_BANDS];
    for i in 0..buckets {
        out[i * ENVELOPE_BANDS + BAND_PEAK] = (peaks[i].clamp(0.0, 1.0) * 255.0) as u8;
        for b in 0..3 {
            let rms = if counts[i] > 0 {
                (squares[i][b] / counts[i] as f64).sqrt()
            } else {
                0.0
            };
            out[i * ENVELOPE_BANDS + BAND_LOW + b] = (rms.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 0.2s silent 44.1kHz stereo 16-bit WAV.
    fn write_wav(path: &Path) {
        write_signal(path, &|_| 0);
    }

    /// Writes the same WAV with a caller-chosen sample at each frame.
    fn write_signal(path: &Path, sample: &dyn Fn(u32) -> i16) {
        let rate = 44_100u32;
        let frames = rate / 5;
        let data_len = frames * 4;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes()); // PCM
        f.extend_from_slice(&2u16.to_le_bytes()); // stereo
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * 4).to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        for frame in 0..frames {
            let s = sample(frame).to_le_bytes();
            f.extend_from_slice(&s);
            f.extend_from_slice(&s);
        }
        std::fs::write(path, f).unwrap();
    }

    /// Guards the decoder feature set.
    ///
    /// rodio splits container readers from codecs: enabling `symphonia-wav`
    /// without `symphonia-pcm` parses a WAV header and then cannot decode a
    /// single sample. That failure only shows up at play time, on the user's own
    /// files, as "cannot decode" — so it is checked here instead.
    #[test]
    fn a_plain_wav_decodes_and_reports_its_duration() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("silence.wav");
        write_wav(&path);

        let file = std::fs::File::open(&path).unwrap();
        let decoder = rodio::Decoder::builder()
            .with_data(std::io::BufReader::new(file))
            .with_seekable(true)
            .build()
            .expect("a 16-bit PCM WAV must decode");

        let duration = rodio::Source::total_duration(&decoder)
            .expect("a WAV of known length must report a duration");
        assert!(
            (150..=250).contains(&(duration.as_millis() as u64)),
            "expected about 200ms, got {duration:?}"
        );
    }

    /// Seeking is what the transport does on every click of the bar; a build
    /// whose decoder cannot seek makes the bar decorative.
    #[test]
    fn a_decoded_wav_can_be_seeked() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("silence.wav");
        write_wav(&path);

        let file = std::fs::File::open(&path).unwrap();
        let mut decoder = rodio::Decoder::builder()
            .with_data(std::io::BufReader::new(file))
            .with_seekable(true)
            .build()
            .unwrap();

        rodio::Source::try_seek(&mut decoder, Duration::from_millis(100))
            .expect("a seekable WAV must accept a seek");
    }

    /// Pulls one band out of an envelope, bucket by bucket.
    fn band(env: &[u8], b: usize) -> Vec<u8> {
        env.chunks(ENVELOPE_BANDS).map(|c| c[b]).collect()
    }

    /// Overall loudness per bucket, 0..1 — the three bands recombined.
    ///
    /// The bands partition the spectrum, so their energies add: this is the
    /// same number a plain full-range RMS would have produced, which is what the
    /// waveform draws its body height from.
    fn loudness(env: &[u8]) -> Vec<f32> {
        env.chunks(ENVELOPE_BANDS)
            .map(|c| {
                let square: f32 = (BAND_LOW..=BAND_HIGH)
                    .map(|b| {
                        let v = c[b] as f32 / 255.0;
                        v * v
                    })
                    .sum();
                square.sqrt()
            })
            .collect()
    }

    /// The envelope must follow the audio, not just come back the right length.
    ///
    /// The signal is silent for its first half and full-scale for its second, so
    /// a correct extraction is flat then loud — which also catches an envelope
    /// built in the wrong order, or one that spread a short file across the
    /// wrong number of buckets.
    #[test]
    fn the_envelope_follows_the_loudness_of_the_audio() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("half.wav");
        let half = 44_100 / 5 / 2;
        write_signal(&path, &|frame| if frame < half { 0 } else { i16::MAX });

        let env = envelope(&path, 100).expect("a plain WAV must yield an envelope");
        assert_eq!(env.len(), 100 * ENVELOPE_BANDS);

        // A constant full-scale signal is DC, which the low band passes whole and
        // the other two reject — so peak and low are the bands that must move.
        // The bucket straddling the transition can be either, so both sides are
        // checked with a bucket of slack.
        for (b, name) in [(BAND_PEAK, "peak"), (BAND_LOW, "low")] {
            let values = band(&env, b);
            assert!(
                values[..49].iter().all(|&v| v == 0),
                "the silent half of {name} must be flat, got {:?}",
                &values[..49]
            );
            assert!(
                values[51..].iter().all(|&v| v > 200),
                "the loud half of {name} must be near full scale, got {:?}",
                &values[51..]
            );
        }
    }

    /// What makes the waveform readable on a limited master.
    ///
    /// A constant full-scale signal and a sparse train of clicks both peak at
    /// full scale, so a peak-only envelope cannot tell them apart — which is
    /// exactly why a club master draws as a solid block. The loudness the bands
    /// add up to must separate them.
    #[test]
    fn band_loudness_distinguishes_dense_material_from_sparse_transients() {
        let dir = tempfile::TempDir::new().unwrap();
        let rate = 44_100u32;

        let dense = dir.path().join("dense.wav");
        write_signal(&dense, &|_| i16::MAX);

        let sparse = dir.path().join("sparse.wav");
        // One full-scale sample every millisecond, silence in between.
        write_signal(&sparse, &|f| {
            if f % (rate / 1000) == 0 {
                i16::MAX
            } else {
                0
            }
        });

        let dense = envelope(&dense, 20).unwrap();
        let sparse = envelope(&sparse, 20).unwrap();

        assert!(
            band(&dense, BAND_PEAK).iter().all(|&v| v > 250)
                && band(&sparse, BAND_PEAK).iter().all(|&v| v > 250),
            "both must peak at full scale — that is the premise"
        );

        let dense_loud = loudness(&dense);
        let sparse_loud = loudness(&sparse);
        assert!(
            dense_loud.iter().all(|&v| v > 0.9),
            "a constant full-scale signal must read loud, got {dense_loud:?}"
        );
        // One full-scale sample in every 44 carries a forty-fourth of the energy,
        // so the body drawn from it is a fraction of the height of the peak halo
        // around it — which is the whole point of keeping both.
        assert!(
            sparse_loud.iter().all(|&v| v < 0.3),
            "sparse clicks must read quiet, got {sparse_loud:?}"
        );
    }

    /// The reason the envelope is split at all: the colour of a DJ waveform is
    /// the balance of these three bands, so a bass line and a hi-hat pattern must
    /// come out of the full extraction path — not just the filterbank — in
    /// different bands.
    #[test]
    fn the_envelope_separates_bass_from_treble() {
        let dir = tempfile::TempDir::new().unwrap();
        let rate = 44_100.0f32;
        let sine = |freq: f32| {
            move |frame: u32| {
                let t = frame as f32 / rate;
                ((2.0 * std::f32::consts::PI * freq * t).sin() * 30_000.0) as i16
            }
        };

        let bass = dir.path().join("bass.wav");
        write_signal(&bass, &sine(60.0));
        let treble = dir.path().join("treble.wav");
        write_signal(&treble, &sine(8_000.0));

        // The first bucket covers the filters' settling time, so it is skipped.
        let of = |p: &Path, b: usize| -> f32 {
            let env = envelope(p, 10).unwrap();
            let v = band(&env, b);
            v[2..].iter().map(|&x| x as f32).sum::<f32>() / (v.len() - 2) as f32
        };

        assert!(
            of(&bass, BAND_LOW) > of(&bass, BAND_HIGH) * 8.0,
            "a 60Hz tone belongs in the low band"
        );
        assert!(
            of(&treble, BAND_HIGH) > of(&treble, BAND_LOW) * 8.0,
            "an 8kHz tone belongs in the high band"
        );
    }

    /// A file that is not audio must fail rather than return a flat envelope: the
    /// caller records the failure so it is not retried, and a silent-looking
    /// waveform would be indistinguishable from a silent track.
    #[test]
    fn a_file_that_is_not_audio_fails_to_yield_an_envelope() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, b"this is not a sound").unwrap();
        assert!(envelope(&path, 16).is_err());
    }
}
