//! What a source knows about one recording.

/// A candidate match for a track, from any source.
///
/// Every field is optional because no two services carry the same ones. Deezer
/// has BPM and a release date but no label; MusicBrainz has labels and catalogue
/// numbers but almost never a BPM; iTunes has genre and a date but neither.
/// Presenting them side by side and letting the user take fields from whichever
/// one has them is the point — which is why nothing here is flattened into a
/// single "best" answer.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackMatch {
    /// Which service found it.
    pub source: &'static str,
    pub title: String,
    /// Credited artists, in order. Joined for display by whoever shows it, so
    /// the separator is a presentation choice rather than baked in here.
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub label: Option<String>,
    pub catalog_number: Option<String>,
    /// ISO date as the source gives it: a year, or a full date.
    pub release_date: Option<String>,
    pub genres: Vec<String>,
    pub bpm: Option<u32>,
    /// Musical key as the source writes it, unnormalised.
    pub key: Option<String>,
    pub isrc: Option<String>,
    pub duration_ms: Option<u64>,
    pub track_no: Option<u32>,
    /// Where this came from, so the user can check it.
    pub url: Option<String>,
    /// Cover art at a size worth embedding, when the source offers one.
    pub art: Option<String>,
    /// How well this matched what was searched for, 0.0 to 1.0.
    pub score: f32,
}

impl TrackMatch {
    /// The credited artists as one string, the way a tag stores them.
    pub fn artist(&self) -> String {
        self.artists.join(", ")
    }

    /// Whether this carries anything worth writing.
    ///
    /// A result with nothing but a title repeats what the file already says, and
    /// offering it as a match to apply would waste the user's attention.
    pub fn is_useful(&self) -> bool {
        self.album.is_some()
            || self.label.is_some()
            || self.release_date.is_some()
            || self.bpm.is_some()
            || self.key.is_some()
            || !self.genres.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artists_join_the_way_a_tag_stores_them() {
        let m = TrackMatch {
            artists: vec!["Skee Mask".into(), "Zenker Brothers".into()],
            ..Default::default()
        };
        assert_eq!(m.artist(), "Skee Mask, Zenker Brothers");
    }

    /// A result that only repeats the title tells the user nothing they did not
    /// already have.
    #[test]
    fn a_match_with_nothing_to_add_is_not_useful() {
        let bare = TrackMatch {
            title: "Czill".into(),
            artists: vec!["Afriqua".into()],
            ..Default::default()
        };
        assert!(!bare.is_useful());

        let with_label = TrackMatch {
            label: Some("Ilian Tape".into()),
            ..bare.clone()
        };
        assert!(with_label.is_useful());

        let with_bpm = TrackMatch {
            bpm: Some(128),
            ..bare
        };
        assert!(with_bpm.is_useful());
    }
}
