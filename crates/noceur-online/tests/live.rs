//! Checks against the real services.
//!
//! All ignored by default: the rest of the suite must run on a machine with no
//! network and must not depend on a third party being up. Run them by hand when
//! changing a request or when a service's format is suspected of having moved:
//!
//! ```text
//! cargo test -p noceur-online -- --ignored --nocapture
//! ```

use noceur_online::{covers, musicbrainz, CoverSize};

#[test]
#[ignore = "reaches the network"]
fn a_real_search_returns_releases() {
    let releases =
        musicbrainz::search_releases("Skee Mask", "Compro").expect("MusicBrainz should answer");
    assert!(!releases.is_empty(), "a well-known record should be found");

    for r in releases.iter().take(3) {
        println!(
            "{:>3}  {:<40} {:<24} {:<10} {}",
            r.score, r.title, r.artist, r.date, r.id
        );
        assert!(
            !r.id.is_empty(),
            "every release needs an id to look art up by"
        );
    }
}

#[test]
#[ignore = "reaches the network"]
fn a_real_cover_downloads_and_is_an_image() {
    let releases = musicbrainz::search_releases("Skee Mask", "Compro").unwrap();

    // Not every release has art, so try a few before concluding anything.
    let mut found = None;
    for r in releases.iter().take(6) {
        match covers::fetch_front(&r.id, CoverSize::Embed) {
            Ok(Some(bytes)) => {
                found = Some((r.clone(), bytes));
                break;
            }
            Ok(None) => continue,
            Err(e) => panic!("fetching {} failed: {e}", r.id),
        }
    }

    let (release, bytes) = found.expect("at least one release should carry a front cover");
    println!(
        "{} — {} bytes, {:?}",
        release.title,
        bytes.len(),
        covers::sniff_mime(&bytes)
    );
    assert!(bytes.len() > 1024, "a 500px cover should not be tiny");
    assert!(
        covers::sniff_mime(&bytes).is_some(),
        "what came back must actually be an image"
    );
}

#[test]
#[ignore = "reaches the network"]
fn a_release_with_no_art_reports_nothing_rather_than_failing() {
    // A well-formed id that is not in the archive.
    let missing = covers::fetch_front("00000000-0000-0000-0000-000000000000", CoverSize::Thumbnail);
    assert!(
        matches!(missing, Ok(None)),
        "a release with no cover is normal, not an error: {missing:?}"
    );
}

#[test]
#[ignore = "reaches the network"]
fn every_source_answers_and_agrees_on_a_well_known_record() {
    let found =
        noceur_online::search_all("Skee Mask", "Compro").expect("some source should answer");
    assert!(!found.is_empty());

    let mut seen: Vec<&str> = found.iter().map(|c| c.source).collect();
    seen.sort_unstable();
    seen.dedup();
    println!("sources answering: {seen:?}");

    for c in found.iter().take(9) {
        println!(
            "  {:<20} {:<34} {:<22} {}",
            c.source, c.title, c.artist, c.detail
        );
        assert!(
            noceur_online::sources::is_allowed(&c.full),
            "every candidate must point at a host this app will download from: {}",
            c.full
        );
    }

    assert!(
        seen.len() >= 2,
        "at least two of the three sources should know this record, got {seen:?}"
    );
}

/// The bytes a pick actually embeds, from a source that is not MusicBrainz.
#[test]
#[ignore = "reaches the network"]
fn a_candidate_from_any_source_downloads_as_an_image() {
    let found = noceur_online::search_all("Skee Mask", "Compro").unwrap();

    for c in found.iter().take(6) {
        match noceur_online::fetch_image(&c.full) {
            Ok(bytes) => {
                println!(
                    "{} -> {} bytes, {:?}",
                    c.source,
                    bytes.len(),
                    noceur_online::covers::sniff_mime(&bytes)
                );
                assert!(bytes.len() > 1024);
                return;
            }
            // Not every candidate has a picture behind it; that is the point of
            // showing several.
            Err(e) => println!("{} had none: {e}", c.source),
        }
    }
    panic!("no candidate produced a downloadable image");
}
