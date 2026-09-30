# Roadmap

Planned work, in the order it will be built. Each step lands on its own and is ticked off here when
it does.

- [x] **1. Keyboard shortcuts overview.** `F1` or `?` opens a searchable list of every shortcut.
      Also on the toolbar menu and as an optional toolbar button.

## Metadata sources

Deezer, iTunes, Discogs, Beatport, Bandcamp and Tidal are in `noceur-online` (MusicBrainz is used only for Cover Art Archive covers). Every new source plugs into
the same `TrackMatch` / `Candidate` types, so the Find metadata and Find album art dialogs pick it
up without changes of their own.

- [x] **2. Beatport.** No API key needed: the search page
      (`beatport.com/search/tracks?q=…`) embeds its results as JSON in `__NEXT_DATA__`. Each
      track carries BPM, key, genre, label, catalogue number, ISRC, mix name, release date and
      1400 px artwork, which covers what a DJ tags. It is not a published API, so the parser is
      pinned by a saved-page test and fails soft (no results, not an error) if the page changes.
- [x] **3. Bandcamp.** No API key needed: search through the site's own autocomplete endpoint
      (`bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic`), then read the track or
      album page's JSON-LD for title, artists, label, release date, track number and artwork.
- [x] **4. Tidal.** Official catalogue API (`openapi.tidal.com/v2`, client-credentials flow).
      The user's own developer app: Client ID and secret go in Settings → Metadata sources, and
      the source is skipped until both are set. Search is `/searchResults?filter[query]=…`, then
      `/tracks?include=albums,artists,genres,albums.coverArt`. Third-party apps get album,
      release date, ISRC, length and a 640 px sleeve; BPM, key and genre are withheld.
- [x] **5. Acoustic fingerprints (AcoustID).** Identify files whose tags and names say nothing.
      Fingerprinted in-process with `rusty-chromaprint` (pure Rust, output matches `fpcalc`):
      the first two minutes, ~0.3 s a file. Find metadata uses it when a file has no artist or
      title tag, or when its tags find nothing; the recording AcoustID names becomes the search
      query, and the picker says so ("By sound: …"). No MusicBrainz rows are shown. The app
      ships its own AcoustID application key, like Picard and beets.
- [x] **6. File-name guess as a fallback.** Find metadata tries the tags, then the
      fingerprint, then `matching::from_file_name`, moving on whenever one finds nothing and
      never sending the same query twice. The name fills only what the tags lack; when the tags
      are complete but found nothing, it is used whole. The picker says "From filename: …" when
      the name was the query.

## Analysis

- [x] **7. BPM detection.** librosa's method ported to Rust (`noceur-audio::tempo`):
      onset-strength envelope (spectral flux on a mel spectrogram) → autocorrelation, scored
      on the first four beats across the range and weighted by a log-normal prior centred on
      125 BPM → then refined by lining up 24 beats, which gives the decimal librosa's
      one-frame resolution cannot. The range (70–180 by default, three other presets in the
      dialog) settles half/double time. Written whole when within 0.1 of a whole BPM,
      otherwise to one decimal. Detect BPM (toolbar and right-click) lists tagged → detected
      per track; the ones that differ start ticked, and only ticked ones are written.
      Measured on 200 tracks sampled across the library (`tests/tempo_real.rs`): 40% match
      the tag; of the rest, every one of ~30 checked against an independent whole-track
      kick-grid fit agreed with the detection, not the tag — a third of the library's BPM
      tags are exactly 1 BPM low. About 1.2 s for a 6½-minute WAV, nearly
      all of it decoding.
- [x] **8. Key detection.** KeyNet (Korzeniowski & Widmer 2018), a convolutional network,
      with OpenKeyScan's retrained MusicalKeyCNN weights, run in Rust (`noceur-audio::key`) with
      the weights embedded (`models/openkeyscan2.bin`, MIT, 1.8 MB). Input is librosa's constant-Q
      spectrogram (105 bins, 24 per octave from 65 Hz, 0.2 s frames), reproduced in Rust to within
      0.2–0.4% at a quarter of the sample rate; the network is nine convolutions written out as
      im2col matrix products. On the 604 GiantSteps test tracks: 76.5 MIREX weighted, 71.5%
      exactly right, against Mixed In Key 10.3's 75.7 / 69.4 and rekordbox 7.12's 65.5 / 56.8
      (`tests/key_giantsteps.rs`). On 300 tracks of a library tagged by Mixed In Key it agrees
      with MIK on 65% (MusicalKeyCNN's own weights: 61%). Written in Camelot. The dialog became
      Detect BPM & key: one decode per file, both fields tag → detected with a box each.

## Tag tools

What Mp3tag actions are used for, built in, so nobody has to write
`$if($eql($regexp(…` to get it. No `.mta` import: each tool is a button with a
preview and, at most, a number or a template picked from a list. The custom
actions stay for anyone who wants title formatting.

- [ ] **9. One-click tag tools.** Each previews old → new and writes only what changes:
      - **Clean file name**: leaves `Artist - Title.ext`. Drops track numbers at either end,
        stray dashes, underscores, doubled spaces and download symbols.
      - **Album artist from artist**: fills an empty album artist with the artist.
      - **Artist into title** / **Artist out of title**: `Artist - Title` in the title field,
        or split back out of it.
      - **Remove "(Original Mix)"**: from title and album.
      - **Remove brackets at the end**: a trailing `(…)` or `[…]` off the title.
      - **Year only**: `2016-03-04` becomes `2016`.
      - **Trim the file name**: the first or last N characters.
      - **Move into folders**: a root and a pattern such as `%artist% - %album% [%year%]`,
        picked from presets or typed, with the moves previewed.
- [ ] **10. Converters on shortcuts, as in Mp3tag.** `Shift+1`–`Shift+5` (ignored while typing
      in a field; `Alt` stays free for AutoHotkey and ratings): Tag → File name (the rename
      dialog), File name → Tag (tags from name), File name → File name (clean file name),
      Text file → Tag (paste a tracklist, one line per selected file) and Tag → Tag (fill one
      field from others, e.g. title from `%artist% - %title%`). Each opens on the last
      pattern used, so a folder is two keys and Enter.

## Browsing

- [ ] **11. Keyboard navigation in the list.** Arrow keys, `Home` / `End`, `Page Up` /
      `Page Down`, `Enter` to open or play, `Backspace` for the parent folder, `Ctrl+A`. Found
      missing while writing the shortcuts overview.

## Quality and loudness

Ideas taken from AudioAuditor's feature list, reshaped for a DJ library that comes largely from
downloads rather than CD rips.

- [ ] **12. Loudness.** Integrated loudness (LUFS, ITU-R BS.1770 / EBU R128) and true peak
      (dBTP, 4× oversampling), as columns and in the quality report. Optionally written as
      ReplayGain track gain and peak, so players that read it can level-match.
- [ ] **13. Damage check.** Files that are broken rather than lossy: decode errors, files cut
      off before their stated length, dropouts (runs of digital silence mid-track), clicks, and
      "24-bit" files whose low 8 bits are always zero (16-bit padded). Part of Check quality, so a
      bad download from Soulseek is caught before it is played out.
- [ ] **14. Clipping after gain.** The clipping check looks for samples at full scale. A track
      that clipped and was then turned down (normalised to −0.3 dB, say) has the same flat tops
      at a lower level; detect runs of identical peak samples at the file's own maximum, and
      report how far below full scale they sit.
- [ ] **15. Library overview.** One page of statistics from the index, no listening history
      needed: track count and total hours, BPM spread, keys on the Camelot wheel, formats and
      sample rates, quality verdicts, top labels, genres and artists, folders with the most
      problems.
- [ ] **16. Maybe: processing markers.** Platinum Notes detection (its spectral fingerprint:
      a boost near the cutoff, a flat top octave, a sharp wall), only if it can be validated on
      files known to have been through it. Also an offline scan for AI-generator provenance
      markers (tags, C2PA manifests), which only finds tracks that declare themselves; no audio is
      uploaded anywhere.

## Identification

- [ ] **17. Shazam lookup.** For tracks AcoustID does not know, which is most new and underground
      electronic music. The fingerprint is ported from SongRec (`marin-m/SongRec`, GPL-3.0,
      pure Rust: a peak-picked spectrogram of about 12 seconds, encoded as Shazam's signature),
      sent through `noceur-online`'s own HTTP client rather than SongRec's libsoup. The result
      carries title, artists, label, release, ISRC and artwork, and goes into the same picker as
      the other sources. It is Shazam's private app API, not a published one: it can change
      without notice and rate-limits by IP, so it is off until enabled in Settings → Metadata
      sources, runs one file at a time, and comes after the tags and AcoustID in Find metadata.

## Background analysis

- [x] **18. BPM and key without asking.** Each track the player loads is measured by one
      low-priority thread while it plays; only what is played, never the rest of its folder.
      The result is kept in the index (`analysis` table, carried across tag writes like the
      waveform), and Detect BPM & key stores what it measures there too. Settings → Files →
      BPM & key: "Add BPM & key automatically to tracks I play" (off by default), then:
      - **Empty fields** (default): BPM and key are written only where the file has none,
        checked against the file itself just before writing. An existing tag is never replaced.
      - **Show only**: nothing is written.
      - **Overwrite**: the file gets what was measured.

      In the list, a measured value stands in, dimmed, where the file has none, and a dot marks
      a cell whose tag differs from the measurement; clicking the dot writes it. The playing
      track is written a second or two after it starts: into a copy that then replaces the
      file, so the player, still reading the original through its open handle, is not
      disturbed, and the transport shows the new BPM and key at once. Dates are preserved as in
      any retag, creation date included.

When a step changes what the app can do, the README's feature list is updated with it.
