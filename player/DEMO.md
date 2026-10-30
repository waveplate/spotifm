# Static player demo and song builder

Two separate standalone pages are generated:

- **`demo-builder.html`** curates recordings, selects LRCLIB lyrics, sets clip
  offsets, and exports `demo-tracks.json`.
- **`demo.html`** presents only the exported selection using the regular
  player's lyric renderer, Milkdrop analyser, and NTSC/VHS effects. Its
  controls select, replay, advance, and seek within the preview; it has no
  song authoring or offset editor.

Audio comes directly from public preview URLs and timed lyrics from
[LRCLIB](https://lrclib.net/docs). The public demo's lyrics are **prefetched**:
it fetches a single static `demo-lyrics.jsonl` file and decodes its base64 LRC
records. It has no live LRCLIB client or network fallback. The authoring tool
can look up lyrics live while you choose recordings. Apple previews are resolved in the browser
with its [documented catalog JSONP API](https://developer.apple.com/library/archive/documentation/AudioVideo/Conceptual/iTuneSearchAPI/Searching.html).
Neither page needs a Spotifm process, Spotify login, API key, or uploaded audio.

## Curate, export, and build

1. Generate the authoring tool:

   ```sh
   ./player/build.sh --offline --builder-only
   ```

2. Open `player/demo-builder.html`. It starts with the bundled manifest or your
   saved browser draft. Use **Reset to bundled songs** to discard the draft
   and start from the manifest used for the latest build.
3. Search any song or artist and add the desired Apple recording. Alternatively,
   use **Add by metadata or preview URL** with a pinned Apple song ID or a
   public HTTPS audio URL. With neither, it searches for an Apple recording
   matching the title and performers. Preview availability depends on the
   provider; a direct audio URL needs CORS support for the public player's
   audio analyser.
4. Play the preview and enter its full-song start time in seconds. Or click
   a lyric line while that line is heard: the builder sets
   `offset = line timestamp − current preview time`.
5. Use **Find other lyric records** to select a particular timed LRCLIB record.
   The selected ID is pinned in the export. Provider lookups are independent,
   so an unavailable audio preview does not prevent selecting lyrics.
6. Remove or reorder songs as needed, then click **Export JSON**. Importing a
   JSON file replaces the builder's draft. Neither editing nor importing
   changes the public demo until you build it.
7. Replace `player/demo-tracks.json` with the export, or pass the exported file
   directly. Prefetch lyrics, then build:

   ```sh
   python3 player/prefetch-demo-lyrics.py --tracks /path/to/demo-tracks.json
   ./player/build.sh --offline --demo-only --demo-tracks /path/to/demo-tracks.json
   ```

   Alternatively, click **Export lyric bundle** in the builder after exporting
   the JSON, without changing the selection. This reuses cached browser lyrics
   before fetching any missing records. Build using both exported files:

   ```sh
   ./player/build.sh --offline --demo-only \
     --demo-tracks /path/to/demo-tracks.json \
     --lyrics-bundle /path/to/demo-lyrics.jsonl
   ```

   Non-offline builds prefetch missing or mismatched bundles automatically.
   `--prefetch-lyrics` explicitly updates the bundle; with `--offline`, it uses
   only the local prefetch cache. `--defer-lyrics` can prepare the HTML while
   a bundle is being downloaded separately; it never enables live lyrics in
   the public demo. Supply the matching bundle before publishing that build.

The build also writes the public demo to **`docs/demo/index.html`** and
**`docs/demo/demo-lyrics.jsonl`**. The existing GitHub Pages site publishes
`/docs` from `develop`, so the API docs stay at
<https://waveplate.github.io/spotifm/> and the demo is served at
<https://waveplate.github.io/spotifm/demo/>. No Pages settings change is needed.
The demo's scripts and styles are embedded in its HTML; the lyric bundle is
its only additional local asset. `--builder-only` and builds with a deferred,
incomplete lyric bundle leave this publishing directory unchanged.

For local use, serve the docs directory instead of double-clicking the HTML:

```sh
python3 -m http.server 8080 --directory docs
```

Open `http://localhost:8080/demo/`. For other static hosts, publish the contents
of `docs/demo/` as the site root, keeping `index.html` and `demo-lyrics.jsonl`
together. The original `player/demo.html` and `player/demo-lyrics.jsonl` outputs
remain available for the player distribution.
Click to play. Each clip advances to the next sample when it ends, returning
to the first sample after the last one. Lyrics follow the audio clock across
pauses, seeks, and song changes. Demo appearance settings are separate from live settings.
Authoring offsets saved by older demo pages are ignored: the manifest is the
source of timing for the public demo. A message points to the settings gear
at the top right of the page. The visible **Previous** and **Next** buttons
also work on touch devices.

`--offline` reuses visualizer libraries embedded in `player/index.html` when
they are absent from the vendor cache. Preview audio, catalog searches, and
optional web fonts still need internet access at runtime. Omit `--demo-only`
to rebuild the live and minimal players too.

## Signed offsets

`previewStartMs` is full-song time at the start of the audio clip, in
milliseconds. The builder displays seconds, preserving millisecond precision.

```text
full-song time = preview audio time + previewStartMs
clip lyric timestamp = full-song lyric timestamp − previewStartMs
```

Positive values select a later section of the full song. **Negative values
are allowed**: `-500` delays the song's zero-second lyrics by half a second.
They do not seek the audio to a negative time. `null` means uncalibrated and
currently uses zero until you set a value in the builder and export again.

The bundled `player/demo-tracks.json` manifest contains these ten recordings,
in the exported order:

| Song | Clip start / offset |
| --- | --- |
| Gucci Gucci — Kreayshawn | 48.6s |
| Contact Your Local Liquidator — Grim Salvo & Bruxism | 33s |
| 1000 Blunts — $uicideboy$ | 30.2s |
| Mind Playing Tricks On Me — Geto Boys | −0.7s |
| 4EVER — Clairo | 48.5s |
| Doctor Worm — They Might Be Giants | 30s |
| Dream A Little Dream Of Me — Ella Fitzgerald & Louis Armstrong | 84s |
| Condemnation — Depeche Mode | 40.5s |
| The Trial — Pink Floyd | 48.5s |
| BLOODSWEAT — $uicideboy$ | 0.5s |

## Lyric lookup and cache

The **prefetcher and authoring tool** try full recording metadata, then timed lyric searches.
Title variants, dollar-sign artist aliases (such as `$uicideboy$` and
`Suicideboys`), diacritics, and collaborator separators are handled. The
automatic picker still requires a matching title and known performers.
Recording duration ranks matches instead of rejecting every result more
than two seconds away. Use the builder's record selector to pin a particular
version when several releases have different timing.

The command-line prefetcher caches successful results in
`player/.demo-lyrics-cache/`. It retries transient HTTP failures, including
503 and 429, and honors bounded `Retry-After` delays. A failed prefetch leaves
any existing bundle intact. `--refresh` refetches cached records, and
`--offline` packages only cached records. All songs must have timed lyrics
before a complete bundle is written.

The bundle starts with a versioned manifest containing a SHA-256 fingerprint
of the track selection, followed by one JSON record per song. Each record has
`track_index`, `source`, `lrclib_id`, and UTF-8 LRC in `lyrics_base64`.
The build validates the fingerprint, selected record IDs, and timed content;
the player validates the selection again before decoding. Negative clip
offsets stay in the song manifest; the bundle stores full-song timestamps.

In the authoring tool, successful timed LRCLIB results are cached in memory and browser storage
under `spotifm_demo_lrclib:` keys. Full-song timestamps are stored, so changing
an offset does not require another lookup. Recording metadata and the pinned
record ID are part of the key. Remove an entry to force a new lookup. Missing
or failed results are not cached; blocked or full storage does not prevent
playback. The builder's draft uses `spotifm_demo_builder_draft:v1`.

The player displays **Lyrics · LRCLIB** while those lyrics are shown. Live
playback displays Spotify or LRCLIB according to the API's `source` field;
live LRCLIB results also persist in Spotifm's disk cache.

## Manifest fields

| Field | Meaning |
| --- | --- |
| `title`, `artist` | Recording title and primary performer; required |
| `album` | Optional full-recording album |
| `artists` | Optional full performer list, including collaborators |
| `lyricsTitles` | Optional accepted LRCLIB title variants for the same recording |
| `durationSec` | Full recording length, not preview length |
| `appleSongId` | Pinned Apple song ID; resolves a public preview without a key |
| `appleSearch` | Search for a matching Apple recording if no ID or URL is supplied |
| `previewUrl` | Direct HTTPS audio URL; an alternative to an Apple lookup |
| `previewStartMs` | Finite signed clip start in milliseconds, or `null` |
| `spotifyId` | Optional Spotify track ID for the source link |
| `coverUrl` | Optional external cover URL |
| `lrclibId` | Optional pinned timed LRCLIB record ID |

Apple catalog results update duration, album, and cover metadata. The builder
exports pinned song IDs rather than resolved Apple CDN URLs. The provided
export pins nine LRCLIB records; Dream A Little Dream Of Me uses metadata
lookup during prefetch. The public demo always reads its static bundle.

Spotify's [`preview_url`](https://developer.spotify.com/documentation/web-api/reference/get-track)
is nullable and deprecated. An embedded Spotify player does not expose
decoded audio to this page's analyser; direct previews allow the visuals to
react. The manifest can use any external audio provider with CORS support.
