//! The last few thousand frames that went to the speakers, for the meters.
//!
//! A VU meter, an oscilloscope and a goniometer all want the same thing: the
//! samples that are being heard right now. The decoder is wrapped in a [`Tap`]
//! that copies every frame it hands the mixer into a small ring, and the UI asks
//! for the tail of that ring each time it draws.
//!
//! **Pre-fader.** The tap sits between the decoder and the player, so the volume
//! slider does not move the meters. That is deliberate: in a browser for DJs the
//! meter answers "how hot is this file?" — whether a master clips, whether a rip
//! is quiet — and a preview turned down to listen quietly must not make a
//! brickwalled track look polite.
//!
//! The ring is written in batches rather than per frame, so the audio thread
//! takes the lock a couple of hundred times a second rather than tens of
//! thousands, and a UI read never stalls it for longer than one copy.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// Frames the ring holds: enough for a 4096-point spectrum, about 90ms at 44.1kHz.
pub const RING_FRAMES: usize = 4096;

/// Frames buffered on the audio thread before the ring is locked.
///
/// Six milliseconds at 44.1kHz — well under a display frame, so batching adds
/// no latency anyone can see.
const BATCH: usize = 256;

struct Ring {
    frames: Vec<[f32; 2]>,
    /// Next slot to write.
    at: usize,
    rate: u32,
}

/// The shared end of the tap: the audio thread writes, the UI reads.
pub struct Scope {
    ring: Mutex<Ring>,
}

impl Default for Scope {
    fn default() -> Self {
        Scope {
            ring: Mutex::new(Ring {
                frames: vec![[0.0; 2]; RING_FRAMES],
                at: 0,
                rate: 44_100,
            }),
        }
    }
}

impl Scope {
    /// Silences the ring, so a new track or a stop does not show the last one.
    pub fn clear(&self) {
        if let Ok(mut r) = self.ring.lock() {
            r.frames.iter_mut().for_each(|f| *f = [0.0; 2]);
            r.at = 0;
        }
    }

    fn push(&self, batch: &[[f32; 2]], rate: u32) {
        let Ok(mut r) = self.ring.lock() else { return };
        r.rate = rate;
        for f in batch {
            let at = r.at;
            r.frames[at] = *f;
            r.at = (at + 1) % RING_FRAMES;
        }
    }

    /// The newest `frames` frames, oldest first, and the rate they play at.
    pub fn snapshot(&self, frames: usize) -> (u32, Vec<[f32; 2]>) {
        let frames = frames.clamp(1, RING_FRAMES);
        let Ok(r) = self.ring.lock() else {
            return (44_100, vec![[0.0; 2]; frames]);
        };
        let start = (r.at + RING_FRAMES - frames) % RING_FRAMES;
        let out = (0..frames)
            .map(|i| r.frames[(start + i) % RING_FRAMES])
            .collect();
        (r.rate, out)
    }
}

/// A source that copies what passes through it into a [`Scope`].
///
/// Mono is written to both sides, so a mono file reads as a centred line on the
/// goniometer and two equal bars on the meter, which is what it is. Past two
/// channels only the front pair is kept.
pub struct Tap<S> {
    inner: S,
    scope: Arc<Scope>,
    channels: u16,
    /// Samples of the frame being assembled.
    frame: [f32; 2],
    channel: u16,
    batch: Vec<[f32; 2]>,
}

impl<S: Source> Tap<S> {
    pub fn new(inner: S, scope: Arc<Scope>) -> Tap<S> {
        let channels = inner.channels().get();
        Tap {
            inner,
            scope,
            channels,
            frame: [0.0; 2],
            channel: 0,
            batch: Vec::with_capacity(BATCH),
        }
    }

    fn flush(&mut self) {
        if !self.batch.is_empty() {
            self.scope.push(&self.batch, self.inner.sample_rate().get());
            self.batch.clear();
        }
    }
}

impl<S: Source> Iterator for Tap<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let Some(sample) = self.inner.next() else {
            self.flush();
            return None;
        };

        if self.channel < 2 {
            self.frame[self.channel as usize] = sample as f32;
        }
        self.channel += 1;
        if self.channel >= self.channels {
            if self.channels == 1 {
                self.frame[1] = self.frame[0];
            }
            self.batch.push(self.frame);
            self.channel = 0;
            if self.batch.len() >= BATCH {
                self.flush();
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Tap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        // What was batched belongs to the old position; drop it rather than
        // draw a few milliseconds of it after the jump.
        self.batch.clear();
        self.channel = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn source(channels: u16, samples: Vec<f32>) -> SamplesBuffer {
        SamplesBuffer::new(
            NonZero::new(channels).unwrap(),
            NonZero::new(48_000).unwrap(),
            samples,
        )
    }

    /// The tap must be transparent: whatever it copies, the mixer gets the same.
    #[test]
    fn samples_pass_through_unchanged() {
        let input: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) - 0.5).collect();
        let tap = Tap::new(source(2, input.clone()), Arc::new(Scope::default()));
        let out: Vec<f32> = tap.collect();
        assert_eq!(out, input);
    }

    #[test]
    fn the_snapshot_is_the_newest_frames_in_order() {
        let scope = Arc::new(Scope::default());
        // Frames numbered 0..600, left positive and right negative.
        let input: Vec<f32> = (0..600)
            .flat_map(|i| [i as f32 / 1000.0, -(i as f32) / 1000.0])
            .collect();
        let _: Vec<f32> = Tap::new(source(2, input), Arc::clone(&scope)).collect();

        let (rate, frames) = scope.snapshot(4);
        assert_eq!(rate, 48_000);
        let expect: Vec<[f32; 2]> = (596..600)
            .map(|i| [i as f32 / 1000.0, -(i as f32) / 1000.0])
            .collect();
        assert_eq!(frames, expect);
    }

    #[test]
    fn mono_is_written_to_both_sides() {
        let scope = Arc::new(Scope::default());
        let _: Vec<f32> = Tap::new(source(1, vec![0.25; 300]), Arc::clone(&scope)).collect();
        let (_, frames) = scope.snapshot(1);
        assert_eq!(frames, vec![[0.25, 0.25]]);
    }

    #[test]
    fn clearing_silences_the_ring() {
        let scope = Arc::new(Scope::default());
        let _: Vec<f32> = Tap::new(source(2, vec![0.5; 600]), Arc::clone(&scope)).collect();
        scope.clear();
        let (_, frames) = scope.snapshot(RING_FRAMES);
        assert!(frames.iter().all(|f| *f == [0.0; 2]));
    }
}
