# Roadmap

Planned work, in the order it will be built. Each step lands on its own and is ticked off here when
it does.

- [x] **1. Keyboard shortcuts overview.** `F1` or `?` opens a searchable list of every shortcut.
      Also on the toolbar menu and as an optional toolbar button.

## Metadata sources

Deezer, iTunes, Discogs, Beatport and Bandcamp are in `noceur-online` (MusicBrainz is used only for Cover Art Archive covers). Every new source plugs into
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
- [ ] **4. Tidal.** Official catalogue API (`openapi.tidal.com`, client-credentials flow). Needs
      a free developer app: the Client ID and secret go in Settings → Sources, and the source
      stays hidden until they are set.
- [x] **5. Acoustic fingerprints (AcoustID).** Identify files whose tags and names say nothing.
      Fingerprinted in-process with `rusty-chromaprint` (pure Rust, output matches `fpcalc`):
      the first two minutes, ~0.3 s a file. Find metadata uses it when a file has no artist or
      title tag, or when its tags find nothing; the recording AcoustID names becomes the search
      query, and the picker says so ("By sound: …"). No MusicBrainz rows are shown. The app
      ships its own AcoustID application key, like Picard and beets.
- [ ] **6. File-name guess as a fallback.** The parser exists (`matching::from_file_name`,
      used by Tags from file name). Use it automatically as the search query when a file has
      no artist/title tags.

## Analysis

- [ ] **7. BPM detection.** librosa is Python and would mean shipping an interpreter, so its
      method is ported to Rust in `noceur-audio` instead: onset-strength envelope (spectral
      flux on a mel spectrogram) → autocorrelation tempogram weighted by a tempo prior → pick
      the peak, with a configurable BPM range (e.g. 70–180) to settle half/double-time.
      Results are offered, not written: the user confirms before `TBPM` changes. Accuracy is
      measured against existing Mixed In Key tags on a real library before it ships.
- [ ] **8. Key detection.** Chroma features against key profiles (Krumhansl/Temperley), or
      libKeyFinder (GPL-3, compatible with this project's licence). Written in the same
      notation as the existing key column (Camelot / Open Key / standard).

## Mp3tag compatibility

- [ ] **9. Multi-step actions and the missing functions.** An action becomes a list of steps,
      each with a target (one field, all tag fields, file name, folder). Adds `$regexp` with
      look-around and `\U` / `\L` / `\E` case changes in the replacement (`fancy-regex`),
      `$num(x,n)`, `$cutLeft`, `$cutRight`, `$mid`, `$eql`, `$trim(x,c)`, `%_counter%` and
      `%_filename%`.
- [ ] **10. Import Mp3tag actions (`.mta`).** Settings → Actions → Import picks a folder or
      files and reports what imported and what could not. The types in use map as:

      | `T=` | Mp3tag action | Noceur step |
      | --- | --- | --- |
      | 1 | Case conversion | case step |
      | 2 | Replace | replace step |
      | 4 | Replace with regular expression | regex step |
      | 5 | Format value | format step (field, file name or folder) |
      | 7 | Guess values | tags-from-name step |
      | 10 | Remove fields except | remove-fields step |

      These six cover all 66 actions in the current Mp3tag setup. The remaining Mp3tag action
      types (export, playlists, cover import/export, split field, …) are skipped with a note.

## Browsing

- [ ] **11. Keyboard navigation in the list.** Arrow keys, `Home` / `End`, `Page Up` /
      `Page Down`, `Enter` to open or play, `Backspace` for the parent folder, `Ctrl+A`. Found
      missing while writing the shortcuts overview.

When a step changes what the app can do, the README's feature list is updated with it.
