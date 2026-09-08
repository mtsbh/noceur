//! Checks the audio fingerprint against real files.
use noceur_audio::identity::audio_hash;

#[test]
#[ignore]
fn a_copy_with_different_tags_is_the_same_audio() {
    let src = std::env::var("NOCEUR_FILE").expect("set NOCEUR_FILE");
    let src = std::path::Path::new(&src);

    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.mp3");
    std::fs::copy(src, &copy).unwrap();

    let before = audio_hash(src).expect("original");
    let plain = audio_hash(&copy).expect("copy");
    assert_eq!(before, plain, "a byte copy must match");

    // Retag the copy, which changes its bytes and its size.
    let edit = noceur_tags::Edit {
        artist: noceur_tags::Change::Set("Somebody Else Entirely".into()),
        title: noceur_tags::Change::Set("A Different Name".into()),
        comment: noceur_tags::Change::Set("x".repeat(4000)),
        ..Default::default()
    };
    noceur_tags::edit::write(&copy, &edit).expect("retag");

    let a = std::fs::metadata(src).unwrap().len();
    let b = std::fs::metadata(&copy).unwrap().len();
    assert_ne!(a, b, "the retag should have changed the size");

    let after = audio_hash(&copy).expect("retagged copy");
    println!("original {before}\nretagged {after}\nsizes {a} vs {b}");
    assert_eq!(before, after, "tags are not part of the audio");
}
