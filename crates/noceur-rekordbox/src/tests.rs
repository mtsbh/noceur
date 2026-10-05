use super::*;

/// A slice of a real file: a track with a grid and cues, a folder of the
/// user's, and Mixed In Key's node appended without fixing ROOT's Count.
const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<DJ_PLAYLISTS Version="1.0.0">
<PRODUCT Name="rekordbox" Version="7.2.19" Company="AlphaTheta" />
<COLLECTION Entries="2">
<TRACK TrackID="235421593" Name="Ertmi - Awaraness" Artist="Ertmi" Kind="AIFF File" Location="file://localhost/X:/ssd/pre-/Ertmi%20-%20Awaraness-3.aiff" Tonality="9A">
<TEMPO Inizio="0.005" Bpm="124.99" Metro="4/4" Battito="1" />
<POSITION_MARK Name="Energy 4" Type="0" Start="0.005" Num="0" Red="40" Green="226" Blue="20" />
</TRACK>
<TRACK TrackID="91133231" Name="Root &amp; Ädnan" Artist="Root" Location="file://localhost/X:/ssd/derp/Root%20%26%20%c3%84dnan.aiff">
</TRACK>
</COLLECTION>
<PLAYLISTS>
<NODE Type="0" Name="ROOT" Count="2">
<NODE Name="CUE Analysis Playlist" Type="1" KeyType="0" Entries="0" />
<NODE Name="ssd" Type="0" Count="1">
<NODE Name="predi" Type="1" KeyType="0" Entries="1">
<TRACK Key="91133231" />
</NODE>
</NODE>
<NODE Name="MIK Cue Points" Type="1" KeyType="0" Entries="1">
<TRACK Key="235421593" />
</NODE>
</NODE>
</PLAYLISTS>
</DJ_PLAYLISTS>"#;

fn list(name: &str, paths: &[&str]) -> Playlist {
    Playlist {
        name: name.into(),
        paths: paths.iter().map(|p| p.to_string()).collect(),
    }
}

/// Describes any path not on `Z:`, a drive that is gone.
fn describe(p: &Path) -> Option<NewTrack> {
    let s = p.to_string_lossy();
    (!s.starts_with("Z:")).then(|| NewTrack {
        name: "New & \"quoted\"".into(),
        artist: "Ärtist".into(),
        kind: "WAV File".into(),
        bpm: Some(126.0),
        tonality: "8A".into(),
        stars: 3,
        ..Default::default()
    })
}

fn tampered(m: &Merged, from: &str, to: &str) -> Merged {
    Merged {
        xml: m.xml.replacen(from, to, 1),
        added: m.added,
        playlists: m.playlists,
        missing: m.missing.clone(),
        cued: m.cued,
        expect: m.expect.clone(),
    }
}

#[test]
fn locations_are_escaped_the_way_rekordbox_escapes_them() {
    for (path, loc) in [
        (
            r"X:\ssd\pre-\derp\raí - ∑k∑s mst24bits.aiff",
            "file://localhost/X:/ssd/pre-/derp/ra%c3%ad%20-%20%e2%88%91k%e2%88%91s%20mst24bits.aiff",
        ),
        (
            r"X:\ssd\oldke\John Tejada - 100% Post-Consumer.aiff",
            "file://localhost/X:/ssd/oldke/John%20Tejada%20-%20100%25%20Post-Consumer.aiff",
        ),
        (
            r"X:\ssd\pre-\derp\Direkt - Difuz live#-2.aiff",
            "file://localhost/X:/ssd/pre-/derp/Direkt%20-%20Difuz%20live#-2.aiff",
        ),
        (
            r"X:\ssd\pre-\Funk E - Litru Ion(kick la mijloc)-2.aiff",
            "file://localhost/X:/ssd/pre-/Funk%20E%20-%20Litru%20Ion(kick%20la%20mijloc)-2.aiff",
        ),
        (
            r"X:\ssd\a'b.aiff",
            "file://localhost/X:/ssd/a%27b.aiff",
        ),
    ] {
        assert_eq!(location(path), loc);
        assert_eq!(location_path(loc), path);
    }
    // Upper-case escapes read the same.
    assert_eq!(
        location_path("file://localhost/C:/a%C3%84.wav"),
        r"C:\aÄ.wav"
    );
}

#[test]
fn a_send_adds_the_folder_and_new_tracks_and_keeps_everything_else() {
    let lists = [
        list(
            "warmup",
            &[
                r"x:\SSD\derp\root & ädnan.aiff",
                r"D:\new\One.wav",
                r"Z:\gone.wav",
            ],
        ),
        list("empty", &[]),
    ];
    let m = merge(SAMPLE, &lists, describe, "2026-10-04").unwrap();
    check(SAMPLE, &m).unwrap();

    assert_eq!(m.added, 1);
    assert_eq!(m.playlists, 2);
    assert_eq!(m.missing, [r"Z:\gone.wav"]);
    // The existing track matched in another case; the new one got the next ID.
    assert!(m.xml.contains("<COLLECTION Entries=\"3\">"));
    assert!(m.xml.contains("TrackID=\"235421594\""));
    assert!(m.xml.contains("Name=\"New &amp; &quot;quoted&quot;\""));
    assert!(m
        .xml
        .contains("Location=\"file://localhost/D:/new/One.wav\""));
    assert!(m.xml.contains("DateAdded=\"2026-10-04\""));
    assert!(m.xml.contains("Rating=\"153\""));
    assert!(m.xml.contains("AverageBpm=\"126.00\""));
    // ROOT counted again: its three children and the folder.
    assert!(m
        .xml
        .contains("<NODE Type=\"0\" Name=\"ROOT\" Count=\"4\">"));
    assert!(m.xml.contains(
        "<NODE Name=\"noceur\" Type=\"0\" Count=\"2\">\n\
         <NODE Name=\"warmup\" Type=\"1\" KeyType=\"0\" Entries=\"2\">\n\
         <TRACK Key=\"91133231\" />\n\
         <TRACK Key=\"235421594\" />\n\
         </NODE>\n\
         <NODE Name=\"empty\" Type=\"1\" KeyType=\"0\" Entries=\"0\" />\n\
         </NODE>\n\
         </NODE>\n\
         </PLAYLISTS>"
    ));
}

#[test]
fn sending_again_replaces_the_folder_and_adds_nothing_twice() {
    let a = [list("a", &[r"D:\new\One.wav"])];
    let first = merge(SAMPLE, &a, describe, "2026-10-04").unwrap();
    let again = merge(&first.xml, &a, describe, "2026-10-05").unwrap();
    check(&first.xml, &again).unwrap();
    assert_eq!(again.added, 0);
    assert_eq!(again.xml, first.xml);

    // A playlist renamed and one deleted in noceur: the folder follows.
    let changed = merge(&first.xml, &[list("b", &[])], describe, "2026-10-05").unwrap();
    check(&first.xml, &changed).unwrap();
    assert!(!changed.xml.contains("Name=\"a\""));
    assert!(changed
        .xml
        .contains("<NODE Name=\"noceur\" Type=\"0\" Count=\"1\">"));
    assert_eq!(changed.xml.matches("Name=\"noceur\"").count(), 1);
    // The track sent before stays in the collection.
    assert!(changed.xml.contains("TrackID=\"235421594\""));

    // An empty folder left by a send of no playlists is found again.
    let none = merge(&first.xml, &[], describe, "2026-10-05").unwrap();
    assert!(none
        .xml
        .contains("<NODE Name=\"noceur\" Type=\"0\" Count=\"0\">\n</NODE>"));
    let back = merge(&none.xml, &a, describe, "2026-10-05").unwrap();
    check(&none.xml, &back).unwrap();
    assert_eq!(back.xml, first.xml);
}

#[test]
fn the_check_catches_a_changed_track_or_playlist() {
    let m = merge(
        SAMPLE,
        &[list("a", &[r"D:\new\One.wav"])],
        describe,
        "2026-10-04",
    )
    .unwrap();
    for (from, to) in [
        ("Start=\"0.005\"", "Start=\"0.006\""),
        ("<TRACK Key=\"91133231\" />\n", ""),
        ("Entries=\"3\"", "Entries=\"4\""),
        ("Count=\"4\"", "Count=\"3\""),
        ("<TRACK Key=\"235421594\" />", "<TRACK Key=\"1\" />"),
        ("rekordbox\" Version", "rekordbox6\" Version"),
    ] {
        assert!(m.xml.contains(from), "{from}");
        assert!(check(SAMPLE, &tampered(&m, from, to)).is_err(), "{from}");
    }
}

#[test]
fn an_empty_start_and_windows_line_ends_both_work() {
    let m = merge(EMPTY, &[list("a", &[r"D:\x.wav"])], describe, "2026-10-04").unwrap();
    check(EMPTY, &m).unwrap();
    assert!(m
        .xml
        .contains("<NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">"));
    assert!(m.xml.contains("<COLLECTION Entries=\"1\">"));

    let crlf = SAMPLE.replace('\n', "\r\n");
    let m = merge(&crlf, &[list("a", &[r"D:\x.wav"])], describe, "2026-10-04").unwrap();
    check(&crlf, &m).unwrap();
    assert!(!m.xml.replace("\r\n", "").contains('\n'));
}

#[test]
fn a_file_that_is_not_rekordbox_xml_is_refused() {
    assert!(merge("<a><b></a>", &[], describe, "x").is_err());
    assert!(merge("<DJ_PLAYLISTS></DJ_PLAYLISTS>", &[], describe, "x").is_err());
}

/// Against the user's real file, which is only read:
/// `NOCEUR_REKORDBOX=D:/db/backups/rekordbox/rekordbox.xml cargo test -p noceur-rekordbox -- --ignored --nocapture`
#[test]
#[ignore]
fn merges_into_the_real_file() {
    let path = std::env::var("NOCEUR_REKORDBOX").expect("NOCEUR_REKORDBOX");
    let old = std::fs::read_to_string(path).unwrap();
    let s = scan(&old).unwrap();
    let known: Vec<String> = s
        .tracks
        .iter()
        .take(3)
        .map(|t| location_path(&t.location))
        .collect();
    let mut paths: Vec<&str> = known.iter().map(String::as_str).collect();
    paths.push(r"D:\nowhere\new.wav");
    let m = merge(&old, &[list("test", &paths)], describe, "2026-10-04").unwrap();
    check(&old, &m).unwrap();
    assert_eq!(m.added, 1);
    println!(
        "{} tracks, {} children under ROOT, +{} bytes",
        s.tracks.len(),
        s.root_children.len(),
        m.xml.len() - old.len()
    );
}

/// A new track goes in with its grid and its cues, each a hot cue and a
/// memory cue, the way Mixed In Key writes them; and the file still reads.
#[test]
fn a_new_track_takes_its_grid_and_cues() {
    let lists = [list("set", &[r"D:\new\One.wav", r"D:\new\Two.wav"])];
    let m = merge(
        SAMPLE,
        &lists,
        |p| {
            let mut t = describe(p)?;
            if p.to_string_lossy().ends_with("One.wav") {
                t.cues = Some(Cues {
                    bpm: 126.0,
                    first: 0.21,
                    marks: vec![
                        Mark {
                            name: "Intro".into(),
                            start: 0.21,
                            rgb: (40, 226, 20),
                        },
                        Mark {
                            name: "Drop".into(),
                            start: 61.1234,
                            rgb: (230, 40, 40),
                        },
                    ],
                });
            }
            Some(t)
        },
        "2026-10-06",
    )
    .unwrap();
    check(SAMPLE, &m).unwrap();
    assert_eq!((m.added, m.cued), (2, 1));
    assert!(m
        .xml
        .contains("<TEMPO Inizio=\"0.210\" Bpm=\"126.00\" Metro=\"4/4\" Battito=\"1\" />"));
    assert!(m.xml.contains(
        "<POSITION_MARK Name=\"Drop\" Type=\"0\" Start=\"61.123\" Num=\"1\" Red=\"230\" Green=\"40\" Blue=\"40\" />"
    ));
    assert!(m
        .xml
        .contains("<POSITION_MARK Name=\"Drop\" Type=\"0\" Start=\"61.123\" Num=\"-1\" />"));
    // The track without cues stays one self-closing element.
    let two = m.xml.find("Two.wav").unwrap();
    assert!(m.xml[two..].starts_with("Two.wav\" Remixer"));
    assert!(m.xml[two..m.xml[two..].find('\n').unwrap() + two].ends_with("/>"));
    // And the whole file is still one rekordbox reads back.
    assert_eq!(known(&m.xml).unwrap().len(), 4);
}
