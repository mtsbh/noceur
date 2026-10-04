//! Short gain ramps at the edges of a track, so switching makes no click.
//!
//! Cutting a waveform off mid-swing, or starting one at full level, is a step
//! from one sample to the next — and a step is a click, loudest on exactly the
//! tracks that start or are stopped on a kick. Going down a list with the
//! sidebar open does this on every row. So each track ramps up from silence
//! when it starts (and after a seek), and ramps down to silence before it is
//! let go rather than being dropped mid-sample.
//!
//! The ramps are counted in frames on the audio thread, so they are the same
//! length whatever the device's buffer size, and too short to be heard as a
//! fade — only as the absence of a click.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

/// Ramp up at the start and after a seek.
const IN_MS: u32 = 10;

/// Ramp down before the source ends itself.
const OUT_MS: u32 = 15;

/// The other end of a [`Fade`]: asks it to ramp down and end.
#[derive(Clone, Default)]
pub struct FadeOut(Arc<AtomicBool>);

impl FadeOut {
    pub fn start(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A source with a ramp in at its start and a ramp out on request.
pub struct Fade<S> {
    inner: S,
    out: FadeOut,
    channels: u16,
    /// Sample within the current frame.
    channel: u16,
    in_frames: u32,
    out_frames: u32,
    /// Frames played since the start or the last seek, up to `in_frames`.
    rising: u32,
    /// Frames left of the ramp out, once it has begun.
    falling: Option<u32>,
    /// The gain of the frame in progress.
    gain: f32,
}

impl<S: Source> Fade<S> {
    pub fn new(inner: S) -> (Fade<S>, FadeOut) {
        let channels = inner.channels().get();
        let rate = inner.sample_rate().get();
        let out = FadeOut::default();
        let fade = Fade {
            inner,
            out: out.clone(),
            channels,
            channel: 0,
            in_frames: (rate * IN_MS / 1000).max(1),
            out_frames: (rate * OUT_MS / 1000).max(1),
            rising: 0,
            falling: None,
            gain: 0.0,
        };
        (fade, out)
    }

    /// The gain for the frame about to start, advancing the ramps by one.
    /// `None` once the ramp out has finished.
    fn step(&mut self) -> Option<f32> {
        if self.falling.is_none() && self.out.0.load(Ordering::Relaxed) {
            self.falling = Some(self.out_frames);
        }
        let mut gain = 1.0;
        if self.rising < self.in_frames {
            gain = self.rising as f32 / self.in_frames as f32;
            self.rising += 1;
        }
        if let Some(left) = self.falling.as_mut() {
            if *left == 0 {
                return None;
            }
            gain *= *left as f32 / self.out_frames as f32;
            *left -= 1;
        }
        Some(gain)
    }
}

impl<S: Source> Iterator for Fade<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;
        // Chosen once per frame and held for its samples, so both sides of a
        // frame get the same gain and the ramp out ends on a frame boundary.
        if self.channel == 0 {
            self.gain = self.step()?;
        }
        self.channel = (self.channel + 1) % self.channels.max(1);
        Some(sample * self.gain)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.inner.size_hint().1)
    }
}

impl<S: Source> Source for Fade<S> {
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
        // A jump is a step too; come in from silence at the new place.
        self.rising = 0;
        self.channel = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    fn ones(frames: usize) -> SamplesBuffer {
        SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(1000).unwrap(),
            vec![1.0; frames * 2],
        )
    }

    /// At 1kHz the ramp in is 10 frames: it starts silent, rises steadily, and
    /// both sides of a frame get the same gain.
    #[test]
    fn it_ramps_in_from_silence() {
        let (fade, _) = Fade::new(ones(100));
        let out: Vec<f32> = fade.collect();
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], 0.0);
        for f in 0..10 {
            assert_eq!(out[2 * f], out[2 * f + 1]);
            assert!((out[2 * f] - f as f32 / 10.0).abs() < 1e-6);
        }
        assert!(out[20..].iter().all(|s| *s == 1.0));
    }

    /// Asked to fade, it ramps down over 15 frames and ends on a frame
    /// boundary, long before the source would have.
    #[test]
    fn it_ramps_out_and_ends() {
        let (mut fade, out) = Fade::new(ones(10_000));
        for _ in 0..100 {
            fade.next();
        }
        out.start();
        let tail: Vec<f32> = fade.collect();
        assert_eq!(tail.len(), 30);
        assert!(tail.windows(3).step_by(2).all(|w| w[2] <= w[0]));
        assert!(tail[0] > 0.9 && tail[29] < 0.1);
    }

    /// A seek comes back in from silence.
    #[test]
    fn a_seek_ramps_in_again() {
        let (mut fade, _) = Fade::new(ones(10_000));
        for _ in 0..100 {
            fade.next();
        }
        fade.try_seek(Duration::from_secs(1)).unwrap();
        assert_eq!(fade.next(), Some(0.0));
    }
}
