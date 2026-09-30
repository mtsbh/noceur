//! Decoding ahead of the output, on a thread of its own.
//!
//! rodio pulls samples from inside the device callback, so a decoder handed to
//! it straight reads the file from inside that callback too. An MP3 is a few
//! kilobytes a second and never noticed. A 24-bit 96kHz FLAC, WAV or AIFF is
//! half a megabyte a second, and at the start of a track its bytes are not in
//! the OS cache yet — often while the waveform for the same file is being read
//! off the same disk. The callback misses its deadline, and Windows fills the
//! gap by replaying the last buffer over and over: a loud buzz for the first
//! moments of every lossless track, exactly when the volume is being judged.
//!
//! So the decoder runs on its own thread, a couple of seconds ahead, and the
//! callback only ever copies from memory. If the disk still falls behind, the
//! callback plays silence and catches up rather than stalling.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// How far ahead the decoder may run, in chunks of [`CHUNK_MS`].
const AHEAD_CHUNKS: usize = 100;

/// Milliseconds of audio in one chunk.
const CHUNK_MS: u32 = 20;

/// Chunks decoded before the first sample is handed over, so a cold disk has
/// a head start rather than being raced by the device from the first frame.
const PREFILL_CHUNKS: usize = 15;

/// How long [`Prefetch::new`] waits for that head start before starting
/// anyway, so a stuck read cannot hold a load open.
const PREFILL_TIMEOUT: Duration = Duration::from_secs(3);

enum Msg {
    /// Interleaved samples, a whole number of frames, from one seek generation.
    Chunk(u64, Vec<Sample>),
    /// The stream ended in that generation.
    End(u64),
}

struct SeekCmd {
    generation: u64,
    to: Duration,
    done: Sender<Result<(), SeekError>>,
}

/// A source fed by a decoder on another thread.
pub struct Prefetch {
    rx: Receiver<Msg>,
    ctl: Sender<SeekCmd>,
    /// Messages already taken off the queue but not yet played.
    pending: VecDeque<Msg>,
    chunk: Vec<Sample>,
    at: usize,
    /// Bumped by every seek; chunks from before it are discarded unplayed.
    generation: u64,
    /// Samples of silence still owed, always a whole frame's worth, so an
    /// underrun never shifts left into right.
    silence: u16,
    ended: bool,
    channels: ChannelCount,
    rate: SampleRate,
    duration: Option<Duration>,
}

impl Prefetch {
    /// Starts decoding `inner` and waits for a head start before returning.
    pub fn new<S>(inner: S) -> Prefetch
    where
        S: Source + Send + 'static,
    {
        let channels = inner.channels();
        let rate = inner.sample_rate();
        let duration = inner.total_duration();

        let per_chunk = ((rate.get() * CHUNK_MS / 1000).max(1) * channels.get() as u32) as usize;
        let (tx, rx) = mpsc::sync_channel(AHEAD_CHUNKS);
        let (ctl, ctl_rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("noceur-decode".into())
            .spawn(move || produce(inner, per_chunk, tx, ctl_rx))
            .expect("spawning the decode thread");

        let mut pending = VecDeque::new();
        let deadline = std::time::Instant::now() + PREFILL_TIMEOUT;
        while pending.len() < PREFILL_CHUNKS {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(msg) => {
                    let end = matches!(msg, Msg::End(_));
                    pending.push_back(msg);
                    if end {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        Prefetch {
            rx,
            ctl,
            pending,
            chunk: Vec::new(),
            at: 0,
            generation: 0,
            silence: 0,
            ended: false,
            channels,
            rate,
            duration,
        }
    }

    fn next_msg(&mut self) -> Result<Msg, TryRecvError> {
        match self.pending.pop_front() {
            Some(msg) => Ok(msg),
            None => self.rx.try_recv(),
        }
    }
}

/// The decode thread: fills the queue, and answers seeks between chunks.
fn produce<S: Source>(mut inner: S, per_chunk: usize, tx: SyncSender<Msg>, ctl: Receiver<SeekCmd>) {
    let mut generation = 0u64;
    let mut ended = false;

    loop {
        // Once the stream has ended there is nothing to do until a seek brings
        // it back; the consumer going away closes the channel and ends this.
        let cmd = if ended {
            match ctl.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => return,
            }
        } else {
            match ctl.try_recv() {
                Ok(cmd) => Some(cmd),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        };
        if let Some(cmd) = cmd {
            generation = cmd.generation;
            let result = inner.try_seek(cmd.to);
            ended = false;
            let _ = cmd.done.send(result);
            continue;
        }

        let mut chunk = Vec::with_capacity(per_chunk);
        chunk.extend(inner.by_ref().take(per_chunk));
        let short = chunk.len() < per_chunk;
        // A decoder that ends mid-frame would leave the channels swapped for
        // anything after it; the partial frame is inaudible, so it is dropped.
        let channels = inner.channels().get() as usize;
        chunk.truncate(chunk.len() - chunk.len() % channels.max(1));

        if !chunk.is_empty() && tx.send(Msg::Chunk(generation, chunk)).is_err() {
            return;
        }
        if short {
            ended = true;
            if tx.send(Msg::End(generation)).is_err() {
                return;
            }
        }
    }
}

impl Iterator for Prefetch {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        loop {
            if self.silence > 0 {
                self.silence -= 1;
                return Some(0.0);
            }
            if self.at < self.chunk.len() {
                let s = self.chunk[self.at];
                self.at += 1;
                return Some(s);
            }
            if self.ended {
                return None;
            }
            match self.next_msg() {
                Ok(Msg::Chunk(g, v)) if g == self.generation => {
                    self.chunk = v;
                    self.at = 0;
                }
                Ok(Msg::End(g)) if g == self.generation => {
                    self.ended = true;
                    return None;
                }
                // Decoded before the latest seek.
                Ok(_) => {}
                // The disk is behind: play a frame of silence rather than wait
                // for it inside the device callback.
                Err(TryRecvError::Empty) => self.silence = self.channels.get(),
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    return None;
                }
            }
        }
    }
}

impl Source for Prefetch {
    fn current_span_len(&self) -> Option<usize> {
        // One format for the whole stream: the decode thread keeps the one it
        // started with.
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }

    fn try_seek(&mut self, to: Duration) -> Result<(), SeekError> {
        self.generation += 1;
        let (done, answer) = mpsc::channel();
        self.ctl
            .send(SeekCmd {
                generation: self.generation,
                to,
                done,
            })
            .map_err(|_| gone())?;

        self.chunk.clear();
        self.at = 0;
        self.silence = 0;
        self.ended = false;
        // Anything queued so far is from before the seek.
        self.pending.clear();

        loop {
            // The decode thread may be blocked on a full queue and unable to
            // see the command until there is room, so make some. Stop at the
            // first message of the new generation: it is already the new audio.
            while let Ok(msg) = self.rx.try_recv() {
                let current = match &msg {
                    Msg::Chunk(g, _) | Msg::End(g) => *g == self.generation,
                };
                if current {
                    self.pending.push_back(msg);
                }
            }
            match answer.recv_timeout(Duration::from_millis(1)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(gone()),
            }
        }
    }
}

fn gone() -> SeekError {
    SeekError::Other(Arc::new(std::io::Error::other(
        "the decode thread has stopped",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    /// Samples numbered from one, so the silence an underrun inserts (zero) is
    /// told apart from the signal.
    fn ramp(frames: usize) -> SamplesBuffer {
        let samples: Vec<f32> = (1..=frames * 2).map(|i| i as f32).collect();
        SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(1000).unwrap(),
            samples,
        )
    }

    /// Whatever goes in comes out, in order, and then it ends. Reading faster
    /// than the decode thread runs inserts silence, whole frames of it.
    #[test]
    fn it_is_transparent() {
        let out: Vec<f32> = Prefetch::new(ramp(50_000)).collect();
        assert_eq!(out.len() % 2, 0, "silence must come in whole frames");
        let signal: Vec<f32> = out.into_iter().filter(|s| *s != 0.0).collect();
        let expect: Vec<f32> = (1..=100_000).map(|i| i as f32).collect();
        assert_eq!(signal, expect);
    }

    /// A seek must drop what was decoded ahead and resume at the new place —
    /// including when the decode thread is blocked on a full queue.
    #[test]
    fn a_seek_discards_what_was_decoded_ahead() {
        // Far more than the queue holds, so the producer is blocked.
        let mut p = Prefetch::new(ramp(1000 * 60));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(p.next(), Some(1.0));
        assert_eq!(p.next(), Some(2.0));

        p.try_seek(Duration::from_secs(30)).unwrap();
        // 30s at 1kHz stereo is sample 60 000; the first after the seek must be
        // a left sample at or near there, not something left over from before.
        let first = p.find(|s| *s != 0.0).unwrap();
        assert!((59_990.0..=60_010.0).contains(&first), "got {first}");
        assert_eq!(first as u64 % 2, 1, "must land on a left sample");
    }

    #[test]
    fn it_reports_the_format_of_the_source() {
        let p = Prefetch::new(ramp(10));
        assert_eq!(p.channels().get(), 2);
        assert_eq!(p.sample_rate().get(), 1000);
    }
}
