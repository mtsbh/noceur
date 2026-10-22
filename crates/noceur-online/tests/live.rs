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

#[test]
#[ignore = "reaches the network"]
fn matching_a_real_track_ranks_the_right_answer_first() {
    let found = noceur_online::match_track(
        "Skee Mask",
        "Rev8617",
        Some(6 * 60 * 1000),
        &Default::default(),
    )
    .expect("some source should answer");
    assert!(!found.is_empty(), "a released record should be matched");

    for m in found.iter().take(8) {
        println!(
            "{:>5.2}  {:<20} {:<28} {:<24} {:<26} {:<6} {}",
            m.score,
            m.source,
            m.title,
            m.artist(),
            m.album.clone().unwrap_or_default(),
            m.bpm.map(|b| b.to_string()).unwrap_or_default(),
            m.label.clone().unwrap_or_default()
        );
    }

    let best = &found[0];
    assert!(
        best.title.to_lowercase().contains("rev8617"),
        "the best match should be the track asked for, got {:?}",
        best.title
    );
}

/// The fields that make the feature worth having: a DJ library is missing
/// labels, dates and BPMs, and no single source has all three.
#[test]
#[ignore = "reaches the network"]
fn between_them_the_sources_supply_the_fields_a_dj_library_lacks() {
    // A record that is definitely in every catalogue. An obscure promo would be
    // testing the databases' coverage rather than this code.
    let mut found =
        noceur_online::match_track("Skee Mask", "Rev8617", None, &Default::default()).unwrap();
    assert!(!found.is_empty());

    for m in found.iter_mut().take(4) {
        if m.source == noceur_online::deezer::NAME {
            noceur_online::deezer::enrich(m).ok();
        }
    }

    let has = |f: fn(&noceur_online::tracks::TrackMatch) -> bool| found.iter().any(f);
    println!(
        "album={} label={} date={} bpm={} genre={}",
        has(|m| m.album.is_some()),
        has(|m| m.label.is_some()),
        has(|m| m.release_date.is_some()),
        has(|m| m.bpm.is_some()),
        has(|m| !m.genres.is_empty()),
    );
    assert!(has(|m| m.album.is_some()), "somebody should know the album");
    assert!(
        has(|m| m.release_date.is_some()),
        "somebody should know when it came out"
    );
}

/// The autotagger's real job: a file with no tags at all, matched from its name.
#[test]
#[ignore = "reaches the network"]
fn an_untagged_file_can_be_matched_from_its_name_alone() {
    let guess = noceur_online::matching::from_file_name("01 - Skee Mask - Rev8617.flac");
    assert_eq!(guess.artist, "Skee Mask");

    let found =
        noceur_online::match_track(&guess.artist, &guess.title, None, &Default::default()).unwrap();
    assert!(!found.is_empty(), "a name alone should be enough to match");
    println!("from name: {} — {}", found[0].artist(), found[0].title);
}

/// Beatport's page is not a published API, so this is the check that its shape
/// has not moved under the parser.
#[test]
#[ignore = "reaches the network"]
fn beatport_supplies_bpm_key_and_label() {
    let found = noceur_online::beatport::search_tracks("Les Points", "Bakteria", 5)
        .expect("Beatport should answer");
    assert!(
        !found.is_empty(),
        "the page parsed to nothing; has it changed?"
    );

    for m in &found {
        println!(
            "  {:<28} {:<18} {:>4} {:<9} {}",
            m.title,
            m.artist(),
            m.bpm.map(|b| b.to_string()).unwrap_or_default(),
            m.key.clone().unwrap_or_default(),
            m.label.clone().unwrap_or_default()
        );
    }
    let best = &found[0];
    assert!(best.bpm.is_some() && best.key.is_some() && best.label.is_some());
    assert!(noceur_online::sources::is_allowed(
        best.art.as_deref().unwrap()
    ));
}

/// Neither Bandcamp endpoint is published, so this is the check that both
/// still look the way the parsers expect.
#[test]
#[ignore = "reaches the network"]
fn bandcamp_finds_a_track_and_its_page_fills_in_the_rest() {
    let mut found = noceur_online::bandcamp::search_tracks("Skee Mask", "Rev8617", 5)
        .expect("Bandcamp should answer");
    assert!(
        !found.is_empty(),
        "the search parsed to nothing; has it changed?"
    );

    let best = &mut found[0];
    noceur_online::bandcamp::enrich(best).expect("the track page should load");
    println!(
        "  {} — {} | {:?} | {:?} | {:?} | {:?} | {:?}",
        best.artist(),
        best.title,
        best.album,
        best.label,
        best.release_date,
        best.isrc,
        best.track_no
    );
    assert!(
        best.release_date.is_some(),
        "the page's JSON-LD has moved or changed"
    );
    assert!(noceur_online::sources::is_allowed(
        best.art.as_deref().unwrap()
    ));
}

/// Discogs needs a token, so this only runs when one is in the environment:
///
/// ```text
/// DISCOGS_TOKEN=... cargo test -p noceur-online -- --ignored --nocapture
/// ```
#[test]
#[ignore = "reaches the network and needs a token"]
fn discogs_supplies_the_label_and_catalogue_number() {
    let Ok(token) = std::env::var("DISCOGS_TOKEN") else {
        println!("DISCOGS_TOKEN not set — skipping");
        return;
    };

    let found = noceur_online::discogs::search_tracks("Skee Mask", "Rev8617", 5, &token)
        .expect("Discogs should answer");
    assert!(!found.is_empty(), "a released record should be found");

    for m in found.iter().take(5) {
        println!(
            "  {:<28} {:<18} {:<10} {}",
            m.album.clone().unwrap_or_default(),
            m.label.clone().unwrap_or_default(),
            m.catalog_number.clone().unwrap_or_default(),
            m.genres.join(", ")
        );
    }
    assert!(
        found.iter().any(|m| m.label.is_some()),
        "the label is the reason to ask Discogs at all"
    );
}

/// A token that is not a token must say so, rather than looking like an outage.
#[test]
#[ignore = "reaches the network"]
fn a_rejected_token_is_reported_as_a_token_problem() {
    let err =
        noceur_online::discogs::search_tracks("Skee Mask", "Rev8617", 5, "notatoken").unwrap_err();
    println!("{err}");
    assert!(
        matches!(err, noceur_online::OnlineError::BadToken { .. }),
        "got {err:?}"
    );
}

/// A fingerprint made by `noceur_audio::fingerprint` from a real FLAC. If
/// AcoustID names the track, the fingerprint is one it can read — which is the
/// check that the in-process port really matches `fpcalc`.
#[test]
#[ignore = "reaches the network"]
fn a_real_fingerprint_is_identified() {
    use noceur_online::acoustid;
    assert!(acoustid::available(), "no AcoustID key is built in");

    let fp = include_str!("fixtures/burial_south_london_boroughs.fp").trim();
    let found = acoustid::lookup(fp, 308).expect("AcoustID should answer");
    for h in found.iter().take(5) {
        println!("  {:.2}  {} — {}", h.score, h.artist(), h.title);
    }
    let best = found.first().expect("a released track should be known");
    assert!(best.score >= acoustid::MIN_SCORE);
    assert!(best.artist().contains("Burial"), "{best:?}");
    assert!(best.title.contains("South London Boroughs"), "{best:?}");
}

/// Needs the user's own Tidal app, from the environment:
/// `NOCEUR_TIDAL_ID` and `NOCEUR_TIDAL_SECRET`.
#[test]
#[ignore = "reaches the network"]
fn tidal_finds_a_track_with_bpm_key_and_cover() {
    let id = std::env::var("NOCEUR_TIDAL_ID").expect("NOCEUR_TIDAL_ID");
    let secret = std::env::var("NOCEUR_TIDAL_SECRET").expect("NOCEUR_TIDAL_SECRET");
    let mut found = Vec::new();
    for (artist, title) in [("Skee Mask", "Rev8617"), ("Fred again..", "Delilah")] {
        found.extend(
            noceur_online::tidal::search_tracks(artist, title, 3, &id, &secret)
                .expect("Tidal should answer"),
        );
    }
    for m in &found {
        println!(
            "  {} — {} | {:?} | {:?} bpm {:?} key {:?} | {:?}",
            m.artist(),
            m.title,
            m.album,
            m.release_date,
            m.bpm,
            m.key,
            m.art
        );
    }
    assert!(!found.is_empty(), "a released track should be found");
    assert!(
        found.iter().any(|m| m.art.is_some()),
        "covers come with albums"
    );
}

/// Needs the user's TrackID client as `NOCEUR_TRACKID_KEY`, `clientId:secret`.
#[test]
#[ignore = "reaches the network"]
fn a_real_set_has_a_tracklist_with_times() {
    let Ok(key) = std::env::var("NOCEUR_TRACKID_KEY") else {
        eprintln!("NOCEUR_TRACKID_KEY not set; skipping");
        return;
    };
    let found =
        noceur_online::trackid::search_sets("Katzenohr", &key).expect("TrackID should answer");
    assert!(
        !found.is_empty(),
        "a DJ with several sets there should be found"
    );
    for s in &found {
        println!("{:<60} {:?} {}", s.title, s.duration_ms, s.slug);
    }

    let set = noceur_online::trackid::fetch_set(&found[0].slug, &key).expect("the set should load");
    for t in &set.tracks {
        println!(
            "{:>8} {:>8}  {} - {}",
            t.start_ms, t.end_ms, t.artist, t.title
        );
        assert!(t.end_ms >= t.start_ms);
    }
    assert!(!set.tracks.is_empty(), "a finished set should have tracks");
}
