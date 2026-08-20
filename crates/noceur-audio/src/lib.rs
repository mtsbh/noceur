//! Preview playback.
//!
//! Playback here is for auditioning while browsing, not for performance: one
//! track at a time, no gapless, no crossfade. The design spec pulled it into the
//! browser sub-project because a seekable waveform needs a player underneath it.

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
    Load { path: PathBuf, play: bool },
    Resume,
    Pause,
    Toggle,
    Stop,
    Seek(Duration),
    Volume(f32),
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

        std::thread::Builder::new()
            .name("noceur-audio".into())
            .spawn(move || run(rx, thread_state))
            .expect("spawning the audio thread");

        Player { tx, state }
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
fn run(rx: mpsc::Receiver<Cmd>, state: Arc<Mutex<Playback>>) {
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
                Cmd::Load { path, play } => match open(&path, sink.mixer(), volume) {
                    Ok((new_player, duration)) => {
                        if !play {
                            new_player.pause();
                        }
                        player = Some(new_player);
                        set(&state, |s| {
                            s.path = Some(path.display().to_string());
                            s.playing = play;
                            s.position_ms = 0;
                            s.duration_ms = duration;
                            s.error = None;
                        });
                    }
                    Err(e) => {
                        player = None;
                        set(&state, |s| {
                            s.path = None;
                            s.playing = false;
                            s.position_ms = 0;
                            s.duration_ms = None;
                            s.error = Some(e.to_string());
                        });
                    }
                },
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
                    if let Some(p) = &player {
                        p.stop();
                    }
                    player = None;
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

/// Opens a file and starts it on the mixer, reporting its duration if known.
fn open(
    path: &Path,
    mixer: &rodio::mixer::Mixer,
    volume: f32,
) -> Result<(rodio::Player, Option<u64>), AudioError> {
    let file = std::fs::File::open(noceur_extended(path)).map_err(|source| AudioError::Open {
        path: path.display().to_string(),
        source,
    })?;

    let decoder = rodio::Decoder::builder()
        .with_data(std::io::BufReader::new(file))
        // The decoder needs a hint on containers whose bytes alone are ambiguous.
        .with_seekable(true)
        .build()
        .map_err(|e| AudioError::Decode {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    let duration = rodio::Source::total_duration(&decoder).map(|d| d.as_millis() as u64);

    let player = rodio::Player::connect_new(mixer);
    player.set_volume(volume);
    player.append(decoder);

    Ok((player, duration))
}

/// Long-path form, so a deeply nested library still opens.
fn noceur_extended(path: &Path) -> PathBuf {
    noceur_fsmodel::path::extended(path)
}

fn set(state: &Arc<Mutex<Playback>>, f: impl FnOnce(&mut Playback)) {
    if let Ok(mut s) = state.lock() {
        f(&mut s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 0.2s silent 44.1kHz stereo 16-bit WAV.
    fn write_wav(path: &Path) {
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
        f.extend_from_slice(&vec![0u8; data_len as usize]);
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
}
