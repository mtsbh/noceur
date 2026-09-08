//! A spectrogram of a whole track, as a grid of levels.
//!
//! What Spek and foobar2000's spectrogram show: time across, frequency up,
//! loudness as colour. It answers at a glance what the quality report answers in
//! words — a lossy file has a ruler-straight line across the top with nothing
//! above it, and a resampled one has that line lower than its own Nyquist.
//!
//! Produced as numbers rather than as a picture. The colour map is a rendering
//! choice, and keeping it in the front end means changing it costs a repaint
//! rather than a re-decode of the track. It is the same arrangement the waveform
//! envelope uses, and for the same reason.

use crate::fft::{fft, hann};
use crate::AudioError;
use std::path::Path;

/// Columns across the width of the image.
///
/// Roughly one per pixel of the strip at a normal window width, so the picture
/// is drawn at about its own resolution rather than stretched to two and a half
/// times it — which is what made it look soft.
pub const COLUMNS: usize = 1024;

/// Frequency bands up the height.
///
/// Eight of the transform's own bins each, which is as fine as a 4096-point
/// window has to give. Taller than the strip, so the picture is scaled down
/// rather than up and a cutoff lands on a sharp edge instead of a gradient.
///
/// Linear rather than logarithmic, deliberately. A log axis is better for
/// reading music and worse for the thing this is for: an encoder's cutoff is a
/// straight horizontal line on a linear axis, and log scaling squeezes the top
/// of the spectrum — exactly where the answer is — into a few pixels.
pub const BANDS: usize = 256;

/// Transform size. Matches the quality analysis, so both read the same detail.
const FFT_SIZE: usize = 4096;

/// Half a window, so consecutive frames overlap and a transient between them is
/// not lost to the gap.
const HOP: usize = FFT_SIZE / 2;

/// Quietest level drawn, in dB below the loudest point of the track.
///
/// Below this everything is the same black. Ninety-six is the dynamic range of
/// sixteen-bit audio, so nothing real is being thrown away.
const FLOOR_DB: f32 = -96.0;

/// Measures the spectrogram of a file.
///
/// Returns `columns * bands` bytes, column by column, each 0 for silence and 255
/// for the loudest point in the track. Levels are relative to that point rather
/// than absolute: the picture is for reading shape, and a quiet track that drew
/// as an empty box would show nothing at all.
pub fn spectrogram(path: &Path, columns: usize, bands: usize) -> Result<Vec<u8>, AudioError> {
    let full = crate::noceur_extended(path);
    let opened = crate::source::open(&full).map_err(|source| AudioError::Open {
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
    let duration = rodio::Source::total_duration(&decoder);

    // A stream that turns out longer than advertised piles into the last column
    // rather than running off the end; one that ends early leaves the tail
    // black. Both beat refusing to draw anything.
    let total = duration
        .map(|d| (d.as_secs_f64() * rate as f64) as usize)
        .filter(|f| *f > 0)
        .unwrap_or(rate as usize * 300);

    let mut grid = Grid::new(columns, bands, total);
    let window = hann(FFT_SIZE);
    let mut block: Vec<f32> = Vec::with_capacity(FFT_SIZE);

    let mut sum = 0f32;
    let mut in_frame = 0usize;
    let mut at = 0usize;

    for sample in decoder {
        sum += sample;
        in_frame += 1;
        if in_frame < channels {
            continue;
        }
        block.push(sum / channels as f32);
        sum = 0.0;
        in_frame = 0;
        at += 1;

        if block.len() == FFT_SIZE {
            // Placed by the middle of the window, which is the moment it
            // describes — using the near edge shifts the whole picture half a
            // window earlier than the sound it came from.
            grid.add(&block, &window, at - FFT_SIZE / 2);
            block.drain(..HOP);
        }
    }

    Ok(grid.finish())
}

/// The same, for samples already in memory.
pub fn from_samples(mono: &[f32], columns: usize, bands: usize) -> Vec<u8> {
    let mut grid = Grid::new(columns, bands, mono.len());
    let window = hann(FFT_SIZE);

    let mut start = 0usize;
    while start + FFT_SIZE <= mono.len() {
        grid.add(
            &mono[start..start + FFT_SIZE],
            &window,
            start + FFT_SIZE / 2,
        );
        start += HOP;
    }
    grid.finish()
}

/// Accumulating power per column and band.
struct Grid {
    columns: usize,
    bands: usize,
    total: usize,
    power: Vec<f64>,
    frames: Vec<u32>,
}

impl Grid {
    fn new(columns: usize, bands: usize, total: usize) -> Grid {
        Grid {
            columns: columns.max(1),
            bands: bands.max(1),
            total: total.max(1),
            power: vec![0.0; columns.max(1) * bands.max(1)],
            frames: vec![0; columns.max(1)],
        }
    }

    /// Folds one window into the column its centre falls in.
    fn add(&mut self, samples: &[f32], window: &[f32], centre: usize) {
        let column = (centre * self.columns / self.total).min(self.columns - 1);

        let mut re: Vec<f32> = samples.iter().zip(window).map(|(s, w)| s * w).collect();
        let mut im = vec![0f32; samples.len()];
        fft(&mut re, &mut im);

        let bins = samples.len() / 2;
        let per_band = (bins / self.bands).max(1);

        for band in 0..self.bands {
            let from = band * per_band;
            let to = ((band + 1) * per_band).min(bins);
            if from >= to {
                break;
            }
            let sum: f64 = (from..to)
                .map(|b| (re[b] * re[b] + im[b] * im[b]) as f64)
                .sum();
            self.power[column * self.bands + band] += sum / (to - from) as f64;
        }
        self.frames[column] += 1;
    }

    /// Turns accumulated power into bytes, scaled against the track's own peak.
    fn finish(self) -> Vec<u8> {
        // Averaged per column first: a column holding twice as many windows as
        // its neighbour is not twice as loud, and without this a track whose
        // length was misreported shows a bright band where the columns bunch up.
        let mut cells = vec![0f64; self.power.len()];
        for column in 0..self.columns {
            let frames = self.frames[column].max(1) as f64;
            for band in 0..self.bands {
                let at = column * self.bands + band;
                cells[at] = self.power[at] / frames;
            }
        }

        let peak = cells.iter().cloned().fold(f64::MIN, f64::max);
        if peak <= 0.0 {
            return vec![0; cells.len()];
        }

        cells
            .iter()
            .map(|p| {
                if *p <= 0.0 {
                    return 0;
                }
                let db = 10.0 * (p / peak).log10();
                if db <= FLOOR_DB as f64 {
                    0
                } else {
                    (((db as f32 - FLOOR_DB) / -FLOOR_DB) * 255.0).round() as u8
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const RATE: u32 = 44_100;
    const COLS: usize = 32;
    const ROWS: usize = 64;

    fn tone(n: usize, hz: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * hz * i as f32 / RATE as f32).sin() * 0.5)
            .collect()
    }

    fn cell(grid: &[u8], column: usize, band: usize) -> u8 {
        grid[column * ROWS + band]
    }

    /// Which band a frequency belongs in, by the same arithmetic the grid uses.
    fn band_of(hz: f32) -> usize {
        let bins = FFT_SIZE / 2;
        let per_band = bins / ROWS;
        let bin = (hz * FFT_SIZE as f32 / RATE as f32) as usize;
        bin / per_band
    }

    #[test]
    fn the_grid_is_the_size_it_was_asked_for() {
        let grid = from_samples(&tone(200_000, 1000.0), COLS, ROWS);
        assert_eq!(grid.len(), COLS * ROWS);
    }

    /// The whole point: a frequency shows up at its own height and nowhere else.
    #[test]
    fn a_tone_lands_in_its_own_band() {
        let grid = from_samples(&tone(200_000, 5000.0), COLS, ROWS);
        let band = band_of(5000.0);

        for column in 0..COLS {
            assert!(
                cell(&grid, column, band) > 200,
                "column {column} should be loud at band {band}"
            );
            // And quiet two bands away, which is far outside the window's own
            // spread at this resolution.
            assert!(
                cell(&grid, column, band + 3) < 80,
                "column {column} leaked upwards"
            );
        }
    }

    #[test]
    fn silence_is_all_zero() {
        let grid = from_samples(&vec![0.0; 200_000], COLS, ROWS);
        assert!(grid.iter().all(|v| *v == 0));
    }

    /// Time runs left to right, which is the other axis and just as easy to get
    /// backwards.
    #[test]
    fn a_tone_that_starts_halfway_fills_only_the_second_half() {
        let mut samples = vec![0.0f32; 100_000];
        samples.extend(tone(100_000, 5000.0));

        let grid = from_samples(&samples, COLS, ROWS);
        let band = band_of(5000.0);

        assert_eq!(cell(&grid, 2, band), 0, "silent at the start");
        assert!(cell(&grid, COLS - 2, band) > 200, "loud at the end");
    }

    /// Levels are relative to the track's own peak, so a quiet recording is
    /// still legible rather than an empty black box.
    #[test]
    fn a_quiet_track_is_scaled_to_itself() {
        let loud = from_samples(&tone(200_000, 5000.0), COLS, ROWS);
        let quiet: Vec<f32> = tone(200_000, 5000.0).iter().map(|s| s * 0.001).collect();
        let quiet = from_samples(&quiet, COLS, ROWS);

        let band = band_of(5000.0);
        assert_eq!(cell(&loud, 5, band), cell(&quiet, 5, band));
    }

    /// A file shorter than one window has nothing to transform, and must come
    /// back as an empty picture rather than a panic.
    #[test]
    fn a_file_too_short_to_transform_is_empty() {
        let grid = from_samples(&tone(100, 1000.0), COLS, ROWS);
        assert_eq!(grid.len(), COLS * ROWS);
        assert!(grid.iter().all(|v| *v == 0));
    }
}
