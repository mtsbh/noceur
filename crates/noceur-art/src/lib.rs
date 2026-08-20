//! Turning embedded cover art into row-sized thumbnails.
//!
//! Rasterising happens here, in Rust, for the same reason waveforms will: a list
//! showing a thumbnail per row across a 40k-file folder cannot afford to hand the
//! frontend full-size JPEGs and ask it to scale them. The web view receives image
//! bytes at exactly the size it is about to draw.

use std::path::Path;

/// The three thumbnail sizes the list offers.
///
/// A closed set rather than an arbitrary number, because every distinct size is a
/// separate cache entry and a separate decode; three is enough to choose between
/// a dense list and a browsable wall of covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Small,
    Medium,
    Large,
}

impl ThumbSize {
    /// Edge length in pixels of the square the thumbnail is drawn into.
    pub fn px(self) -> u32 {
        match self {
            ThumbSize::Small => 22,
            ThumbSize::Medium => 44,
            ThumbSize::Large => 88,
        }
    }

    /// Parses the size from the form the frontend puts in a URL.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "small" => Some(ThumbSize::Small),
            "medium" => Some(ThumbSize::Medium),
            "large" => Some(ThumbSize::Large),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThumbSize::Small => "small",
            ThumbSize::Medium => "medium",
            ThumbSize::Large => "large",
        }
    }

    /// Every size, so a caller can pre-generate or invalidate the whole set.
    pub fn all() -> [ThumbSize; 3] {
        [ThumbSize::Small, ThumbSize::Medium, ThumbSize::Large]
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ArtError {
    #[error("cannot read tags of {path}: {source}")]
    Tags {
        path: String,
        #[source]
        source: noceur_tags::MetaError,
    },
    #[error("embedded artwork is not a decodable image: {0}")]
    Decode(#[source] image::ImageError),
    #[error("cannot encode thumbnail: {0}")]
    Encode(#[source] image::ImageError),
}

/// A rendered thumbnail. Always PNG, always square, always `size.px()` a side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub size: ThumbSize,
    pub png: Vec<u8>,
}

/// Renders the front cover of `path` at `size`, or `None` if it has no artwork.
pub fn thumbnail(path: &Path, size: ThumbSize) -> Result<Option<Thumb>, ArtError> {
    let art = noceur_tags::artwork(path).map_err(|source| ArtError::Tags {
        path: path.display().to_string(),
        source,
    })?;

    let Some(art) = art else { return Ok(None) };
    render(&art.data, size).map(Some)
}

/// Renders raw image bytes into a square thumbnail.
///
/// Split out from [`thumbnail`] so the scaling rules can be tested without
/// writing an audio file first.
pub fn render(bytes: &[u8], size: ThumbSize) -> Result<Thumb, ArtError> {
    // The container's declared MIME type is not trusted: files are routinely
    // tagged with a PNG mime carrying JPEG bytes and vice versa. Sniff instead.
    let img = image::load_from_memory(bytes).map_err(ArtError::Decode)?;

    let px = size.px();
    // Cover art is square far more often than not, but not always. Fill the
    // square and crop the overflow rather than letterboxing: a row of thumbnails
    // with inconsistent bars in them reads as broken.
    let scaled = img.resize_to_fill(px, px, image::imageops::FilterType::Lanczos3);

    let mut png = Vec::new();
    scaled
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(ArtError::Encode)?;

    Ok(Thumb { size, png })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbImage};
    use std::io::Cursor;

    /// Encodes a solid-colour image of the given dimensions.
    fn source(w: u32, h: u32, fmt: ImageFormat) -> Vec<u8> {
        let img = RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgb([200, 40, 40])
            } else {
                image::Rgb([40, 40, 200])
            }
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), fmt)
            .unwrap();
        out
    }

    fn dimensions(png: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(png).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn every_size_renders_a_square_of_its_own_edge_length() {
        let src = source(600, 600, ImageFormat::Jpeg);
        for size in ThumbSize::all() {
            let t = render(&src, size).unwrap();
            assert_eq!(
                dimensions(&t.png),
                (size.px(), size.px()),
                "{:?} was not square at its own size",
                size
            );
        }
    }

    /// A non-square cover must still come out square, or the column ragged.
    #[test]
    fn a_wide_cover_is_cropped_to_a_square_not_letterboxed() {
        let src = source(1200, 400, ImageFormat::Jpeg);
        let t = render(&src, ThumbSize::Medium).unwrap();
        assert_eq!(dimensions(&t.png), (44, 44));
    }

    #[test]
    fn a_tall_cover_is_cropped_to_a_square() {
        let src = source(300, 900, ImageFormat::Png);
        let t = render(&src, ThumbSize::Large).unwrap();
        assert_eq!(dimensions(&t.png), (88, 88));
    }

    /// Files routinely declare the wrong mime for their embedded picture, so the
    /// decoder sniffs the bytes rather than believing the tag.
    #[test]
    fn png_and_jpeg_sources_both_decode() {
        for fmt in [ImageFormat::Png, ImageFormat::Jpeg] {
            let src = source(200, 200, fmt);
            assert!(render(&src, ThumbSize::Small).is_ok(), "{fmt:?} failed");
        }
    }

    #[test]
    fn undecodable_bytes_are_an_error_not_a_panic() {
        let err = render(b"this is not an image at all", ThumbSize::Small);
        assert!(matches!(err, Err(ArtError::Decode(_))));
    }

    #[test]
    fn sizes_round_trip_through_their_url_form() {
        for size in ThumbSize::all() {
            assert_eq!(ThumbSize::parse(size.as_str()), Some(size));
        }
        assert_eq!(ThumbSize::parse("enormous"), None);
    }
}
