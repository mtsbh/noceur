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

- [x] **9. One-click tag tools.** Each previews old → new and writes only what changes.
      The broom (Clean up) renames at once, no dialog (Ctrl+Z undoes): the file name down to
      `Artist - Title`, without track numbers at either end, `(Original Mix)`, underscores,
      site names, doubled or `–` dashes and stray symbols. The rest are in the Actions dialog
      (toolbar button, or Actions… on right-click): one tick list, any number at once, run in
      order with one combined preview:
      - **File name**: Cut the first or last N characters.
      - **Tags**: Clean (underscores, site names, spacing), Remove (Original Mix) from title
        and album, Remove brackets at the end, Tidy featuring, Artist out of title, Artist
        into title, Album artist from artist, Year only (`2016-03-04` → `2016`).
      - **Move into folders…** (a second view): one Mp3tag-style path such as
        `E:\music\%artist% - %album% '['%year%']'`, with presets; the file keeps its name.

      Own buttons (Settings → Actions) gain Fill if empty, Remove text, Remove brackets at the
      end and Cut N, all plain boxes; the expression language gains Mp3tag's `$regexp`,
      `$cutLeft`, `$cutRight`, `$eql`/`$neql` and `%_filename%`, so pasted Mp3tag strings run.
- [x] **10. Converters on shortcuts.** One key each, no dialog: `Alt+F1` names the selected
      files `Artist - Title` from their tags (Ctrl+Z undoes), `Alt+F2` fills artist and title
      from `Artist - Title` file names (names that do not fit are skipped and counted), and
      `Alt+F3` opens Find metadata.

## Browsing

- [x] **11. Keyboard navigation in the list.** Arrow keys (all four in the icon view),
      `Home` / `End`, `Page Up` / `Page Down`, with `Shift` to extend the selection; `Enter`
      opens a folder or plays a track, `Backspace` goes to the parent folder, `Ctrl+A` selects
      all. Rows not loaded yet are read on the way, so `End` works in a folder of any size.

## Quality and loudness

Ideas taken from AudioAuditor's feature list, reshaped for a DJ library that comes largely from
downloads rather than CD rips.

- [x] **12. Loudness.** Integrated loudness (LUFS, ITU-R BS.1770-4: K-weighting, 400 ms
      blocks, absolute and relative gates) and true peak (dBTP, 4× polyphase oversampling),
      measured in Check quality's one pass over the file (`noceur-audio::loudness`). In the
      report beside each track, and kept in the index (`loudness` table, carried across tag
      writes) for Write ReplayGain. Shown only in the report, not as list columns. Write ReplayGain (in the report) puts
      `REPLAYGAIN_TRACK_GAIN` (to −18 LUFS) and `REPLAYGAIN_TRACK_PEAK` in the measured files.
      Matches ffmpeg's `ebur128` to 0.1 LU on WAV, FLAC and MP3, and passes EBU Tech 3341 cases
      1–3. Symphonia clips MP3 output at full scale, so a lossy file whose decode reaches it
      shows its true peak as a lower bound (`≥ +0.5`).
- [x] **13. Damage check.** Files that are broken rather than lossy, found in Check quality's one
      pass (`noceur-audio::damage`), which now decodes with symphonia directly: rodio skips
      undecodable packets silently, which is right for playback and hides exactly this.
      Reported beside the verdict in the report (damaged files first, with their own tally),
      and kept in the index (`damage` table, carried across tag writes) for a later library
      overview; like loudness, it is not a list column. Finds: audio missing against the stated length (sample-exact for FLAC, WAV and AIFF,
      so a corrupt FLAC frame symphonia resyncs past shows up too; MP3 only with a LAME/Info
      tag), decode errors, dropouts (digital silence with the music at full level on both sides,
      so DAW-faded stops are left alone), isolated single-sample clicks (not when the waveform
      keeps ringing, not on a pulse train, not when a same-sign twin recurs anywhere in the
      track), and 16-bit audio in a 24-bit file. Tuned on the 207 lossless files in `slsk`:
      first pass flagged 204, mostly drum transients as clicks; the shipped rules flag a
      handful, each a real bad sample, edit glitch or padded file on inspection. Symphonia's
      AIFF reader decodes a trailing `ID3 ` chunk as a few frames of audio; a gap must be followed
      by 100 ms of sound to count, so that is not reported as a dropout. MP3s commonly end in
      tag bytes or padding the decoder trips over once the music is over (one error in most of
      100 sampled downloads), so decode errors in the last second, or past the stated end, are
      not counted. The report gained a header row (Claimed bitrate, Actual bitrate, Cutoff,
      Clip, LUFS, Peak), an Actual bitrate column with the bitrate the spectrum implies, and shows any clipping, red once it
      is enough to hear.
- [x] **14. Clipping after gain.** A track that clipped and was then turned down (normalised
      to −0.3 dB, say) keeps its flat tops at a lower level. Check quality now also counts runs
      of identical samples, per channel at 16-bit resolution, at every level within 1 dB of the
      file's own peak, and reports the highest one with a run of 8 or more. The Clip column
      shows it with `↓`, and its tip says where the tops sit ("Clipped, then turned down to
      −0.10 dB"). Clipping at full scale still wins when there is any. A deep bass crest repeats
      a 16-bit value two or three times, not eight, so round peaks are left alone. On the 210
      files in `slsk`: 13 clipped at full scale, 8 clipped and turned down (−0.02 to −0.5 dB,
      runs of 23 to 237 identical samples). Lossy files rarely show it: the decoder does not
      rebuild a flat top flat.
- [x] **15. Library overview.** One page of statistics from the index, no listening history
      needed: track count and total hours, BPM spread, keys on the Camelot wheel, formats and
      sample rates, quality verdicts, top labels, genres and artists, folders with the most
      problems. Opens from the toolbar's chart button or `Ctrl+I`, for the whole library or
      (ticked) only the current folder. Measured BPM and key stand in where the tag is empty.
      The BPM chart spans 60–200 and counts the rest as outside, since a few tags read near
      1000. Quality shows what Check quality has stored: tracks checked, damaged, unreadable,
      and a LUFS spread; there is also a count of tracks missing artist, title, genre, BPM, key
      or cover art. Top lists ignore case and stray spaces. Clicking a problem folder opens
      it. The record label is now read into the index (schema 13), so labels fill in as
      folders are re-read. The query takes about 140 ms on 991 tracks.
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
      disturbed, and the transport shows the new BPM and key at once. A copy left behind by a crash
      mid-write is cleared when its folder is next opened: deleted beside an intact original,
      renamed back if the original is gone. Dates are preserved as in
      any retag, creation date included.

## Sidebar

- [ ] **19. Track sidebar.** A panel docked on the right, opened and closed by an arrow button
      on the window's right edge (and a shortcut); open or closed is remembered. Top to bottom:
      - **Album art**, square, the full width of the panel.
      - **Tag panel** for the selected track: file facts that cannot be edited (format, bitrate,
        length, sample rate), then the music fields (album, artists, genre, title, track
        number, year, album artist, BPM, key, label…), edited in place with Apply / Cancel.
        Genres (and perhaps artist, label) as links that list the library's tracks with that
        value.
      - **Search buttons**: the track's `Artist - Title` on Google, YouTube, Discogs,
        Beatport, Bandcamp, SoundCloud and others, each opening the browser.

      While the sidebar is open, selecting a track starts playing it, so going down the list
      is listening through it; with the sidebar closed, selecting does not play, as now.
- [ ] **20. Send to Telegram.** A button (toolbar, right-click, and the sidebar) that hands the
      selected files to Telegram Desktop, which comes to the front with its chat picker; the user
      picks the chat and sends. Done the way Explorer's own "Send to → Telegram" does it
      (`Telegram.exe -sendpath <file>`), so it goes from the user's own account: no bot, no
      setup, and the usual 2 GB limit rather than a bot's 50 MB. Telegram is found where it is
      installed (or through the Windows share sheet for the Store version); the button is hidden
      when it is not.

When a step changes what the app can do, the README's feature list is updated with it.
