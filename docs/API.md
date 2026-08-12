# API

Spotifm serves the same API over HTTP on port `3333` and HTTPS on port `3443` by
default. The automatically generated certificate is self-signed, so clients
must trust it explicitly or use a certificate supplied with
`tls_cert`/`tls_key`.

Examples marked as live were fetched from `https://radio.etalon.cc` on May 31, 2026. Values will vary on your own instance.

Most error responses use:

```json
{ "error": "..." }
```

Playlist names accepted by the mutating playlist endpoints must be ASCII alphanumeric.

## Common payloads

### `TrackItem`

This is the canonical track shape used for playlist entries and queue entries.

```json
{
  "track_id": "1GbtB4zTqAsyfZEsm1RZfx",
  "track_name": "Blinding Lights",
  "artists": ["The Weeknd"],
  "queue_idx": 0,
  "artist_ids": ["1XyoAE8jLE1Z1eUzyUsj7C"],
  "album_id": "4yP0hdKOZPNshxUOjY0cZj",
  "album_name": "After Hours",
  "cover_url": "https://i.scdn.co/image/ab67616d0000b2730d...",
  "playlist_id": "37i9dQZF1DXcBWIGoYBM5M",
  "playlist_name": "Today's Top Hits"
}
```

Notes:

- `queue_idx` is `null` for normal playlist entries and an integer for transient queued items.
- `cover_url`, `album_id`, and `album_name` are present when Spotify provided them.
- `playlist_id` and `playlist_name` are present for tracks resolved from a Spotify playlist.

### `NowPlaying`

This is the canonical playback snapshot returned by `/np`, `/play/<category>/<id>`, `/play/<category>?q=<query>`, `/skip`, `/skip/<category>`, `/queue/<category>/<id>`, `/queue/<category>?q=<query>`, and `/next` when a next track exists.

Live example from `GET /np`:

```json
{
  "status": "playing",
  "track_id": "4c7Fu6BSxiHiYrjoTHlAuY",
  "track_name": "Keep The groovin'",
  "artists": ["YUZO KOSHIRO"],
  "artist_ids": ["6Tvw2YNOLrcxATPGnFVTzz"],
  "album_id": "22mDiQ83M2QjLM9KHsHQ9z",
  "album_name": "Streets of Rage: Perfect Soundtrack",
  "track_duration_ms": 200746,
  "position_ms": 79126,
  "listeners": 0,
  "active_playlist": "streetsofrage",
  "cover_url": "https://i.scdn.co/image/ab67616d0000b2737a14c44b2aa2b6f5a85664db"
}
```

Notes:

- `listeners` is the current live stream subscriber count.
- `position_ms` is a point-in-time snapshot. On mutation endpoints it is commonly `0` or `null`.
- `track_duration_ms`, `active_playlist`, and `cover_url` are omitted when unavailable.
- `/next` uses the same shape with `status: "next"`.

### `ActivePlaylistResponse`

Used by the WebSocket `Playlist` message.

```json
{
  "name": "default",
  "tracks": [
    {
      "track_id": "1GbtB4zTqAsyfZEsm1RZfx",
      "track_name": "Blinding Lights",
      "artists": ["The Weeknd"],
      "queue_idx": null,
      "artist_ids": ["1XyoAE8jLE1Z1eUzyUsj7C"],
      "album_id": "4yP0hdKOZPNshxUOjY0cZj",
      "album_name": "After Hours",
      "cover_url": "https://i.scdn.co/image/ab67616d0000b2730d..."
    }
  ]
}
```

## OAuth

### `GET /oauth`

Authorization helper served on the API port.

Behavior:

- If Spotifm has not finished OAuth authorization yet, this endpoint responds with an HTTP redirect to the Spotify authorization URL.
- Once OAuth is already complete, this endpoint returns a small HTML page saying OAuth is already authorized.

Notes:

- This is a convenience entrypoint only. The actual Spotify callback still goes to the configured `redirect_uri`.
- This is especially useful for headless or Docker deployments where opening a local browser from the CLI is inconvenient.

## Search


### `GET /search/<category>/<limit>?q=<query>`

`category` is one of `track`, `album`, `artist`, or `playlist`.

Notes:

- `limit` must be between `1` and `50`.
- Unknown categories return `400`.
- The response is an array whose item shape depends on the category.

Live example from `GET /search/track/2?q=daft%20punk`:

```json
[
  {
    "track_id": "0DiWol3AO6WpXZgp0goxAV",
    "uri": "spotify:track:0DiWol3AO6WpXZgp0goxAV",
    "track_name": "One More Time",
    "duration_ms": 320357,
    "explicit": false,
    "popularity": 84,
    "artists": ["Daft Punk"],
    "artist_ids": ["4tZwfgrHOc3mvqYlEYSvVi"],
    "album_id": "2noRn2Aes5aoNVsU6iWThc",
    "album_name": "Discovery",
    "album_artists": ["Daft Punk"],
    "cover_url": "https://i.scdn.co/image/ab67616d0000b2731e81bff9807a9e629fce5ade",
    "preview_url": null
  },
  {
    "track_id": "09TlxralXOGX35LUutvw7I",
    "uri": "spotify:track:09TlxralXOGX35LUutvw7I",
    "track_name": "End of Line",
    "duration_ms": 156486,
    "explicit": false,
    "popularity": 70,
    "artists": ["Daft Punk"],
    "artist_ids": ["4tZwfgrHOc3mvqYlEYSvVi"],
    "album_id": "3AMXFnwHWXCvNr5NCCpLZI",
    "album_name": "TRON: Legacy - The Complete Edition (Original Motion Picture Soundtrack)",
    "album_artists": ["Daft Punk"],
    "cover_url": "https://i.scdn.co/image/ab67616d0000b2738323143296ff7b2801e32789",
    "preview_url": null
  }
]
```

Category-specific item shapes:

- `track`: `ExtendedTrack[]`
- `album`: objects with `album_id`, `uri`, `album_name`, `artists`, `artist_ids`, `cover_url`
- `artist`: objects with `artist_id`, `uri`, `artist_name`, `genres`, `popularity`, `cover_url`
- `playlist`: objects with `playlist_id`, `uri`, `playlist_name`, `owner`, `total_tracks`, `cover_url`

## Playback

### `GET /np`

Returns the current `NowPlaying` snapshot.


### `GET /play/<category>/<id>?playlist=<playlist_name>`

`category` is one of `track`, `album`, `artist`, or `playlist`.

Behavior:

- If `playlist` is omitted, the resolved tracks are inserted into the default playlist immediately after the current track and playback jumps to the first resolved track.
- If `playlist` is provided, the named playlist is loaded or created on disk, the resolved tracks are appended, the active playlist switches to it, and playback starts there.

Response: `NowPlaying`

Errors:

- `400` for an invalid category or invalid playlist name
- `400` if no tracks could be resolved
- the corresponding Spotify status, or `502` for other Spotify/API failures

### `GET /play/<category>?q=<query>&playlist=<playlist_name>`

Searches Spotify in the given category, uses the first result, and then behaves like `GET /play/<category>/<id>`.

Response: `NowPlaying`

### `GET /skip`

Skips to the next available track with a single playback load.

Response: `NowPlaying`

### `GET /skip/<n>`

Skips by `n` tracks by advancing the in-memory queue/playlist cursor to the final target and loading that track once.

Notes:

- positive `n` skips forward and consumes transient queued tracks first
- negative `n` skips backward through the normal playlist/library order and leaves transient queued tracks intact
- `n` must not be `0`
- if the playlist runs out during the skip sequence, the final response is the normal idle `NowPlaying` payload

Response: `NowPlaying`

### `GET /skip/<category>`

`category` is `album` or `artist`.

Skips forward in memory until the next track that does not share the current album or artist grouping, then loads only that final track.

Response: `NowPlaying`

### `GET /next`

Returns the upcoming track using the same shape as `NowPlaying`, with `status: "next"`.

Example:

```json
{
  "status": "next",
  "track_id": "3Mv6pY1vvRbVBjuccDjin1",
  "track_name": "Beatnik On The Ship",
  "artists": ["YUZO KOSHIRO"],
  "artist_ids": ["6Tvw2YNOLrcxATPGnFVTzz"],
  "album_id": "22mDiQ83M2QjLM9KHsHQ9z",
  "album_name": "Streets of Rage: Perfect Soundtrack",
  "position_ms": null,
  "listeners": 0,
  "active_playlist": "streetsofrage",
  "cover_url": "https://i.scdn.co/image/ab67616d0000b2737a14c44b2aa2b6f5a85664db"
}
```

If there is no next track, the REST endpoint returns:

```json
{ "error": "No next track" }
```

with HTTP `404`.

## Queue

### `GET /queue`

Returns the transient queue only, sorted by `queue_idx`.

Example:

```json
[
  {
    "track_id": "1GbtB4zTqAsyfZEsm1RZfx",
    "track_name": "Blinding Lights",
    "artists": ["The Weeknd"],
    "queue_idx": 0,
    "artist_ids": ["1XyoAE8jLE1Z1eUzyUsj7C"],
    "album_id": "4yP0hdKOZPNshxUOjY0cZj",
    "album_name": "After Hours",
    "cover_url": "https://i.scdn.co/image/ab67616d0000b2730d..."
  }
]
```

### `GET /queue/<category>/<id>`

Queues tracks resolved from a track, album, artist, or playlist ID without interrupting the current track.

Response: `NowPlaying` with `status: "queued"`

### `GET /queue/<category>?q=<query>`

Searches Spotify in the given category, uses the first result, and then behaves like `GET /queue/<category>/<id>`.

Response: `NowPlaying` with `status: "queued"`

### `DELETE /queue/<track_id>`

Removes a track from the transient queue and returns the updated queue as `TrackItem[]`.

If the track is not present in the queue, returns `404` with:

```json
{ "error": "Track is not in the active transient queue" }
```

## Playlists

### `GET /playlist`

Returns the current active playlist's tracks as `TrackItem[]`.

### `GET /playlists`

Returns detailed information about all saved playlists on disk.

Example:

```json
[
  {
    "name": "default",
    "num_tracks": 42,
    "cover_urls": ["https://i.scdn.co/..."],
    "artists": ["Daft Punk", "Kraftwerk"],
    "last_modified": 1672531199
  }
]
```


### `GET /playlist/<name>`

Returns the named playlist's tracks as `TrackItem[]`.


### `GET /playlist/switch/<name>`

Switches the active playlist and returns its full track list as `TrackItem[]`.

If the named playlist does not exist yet, it is created implicitly as an empty playlist and then activated.

### `GET /playlist/shuffle`
### `GET /playlist/shuffle/<name>`

Shuffles either:

- the current active playlist with `GET /playlist/shuffle`
- the named playlist with `GET /playlist/shuffle/<name>`

Response: the shuffled playlist as `TrackItem[]`

If a named playlist does not exist yet, it is created implicitly and the endpoint returns an empty list.

### `GET /playlist/sort?by=<artist|album|playlist>`
### `GET /playlist/<name>/sort?by=<artist|album|playlist>`

Sorts either:

- the current active playlist with `GET /playlist/sort?by=...`
- the named playlist with `GET /playlist/<name>/sort?by=...`

Response: the sorted playlist as `TrackItem[]`

`by=playlist` sorts by source Spotify playlist metadata when present. Tracks without source playlist metadata sort after tracks that have it.

If a named playlist does not exist yet, it is created implicitly and the endpoint returns an empty list.

### `DELETE /playlist/track/<track_id>`

Removes a track from the current active playlist and returns the updated `TrackItem[]`.

### `GET /playlist/track/<track_id>/play`

Immediately starts the selected track from the current active playlist without changing playlists or reordering its tracks.

Response: `NowPlaying` with `status: "playing"`.

Returns `404` when the track is not part of the active playlist.

### `POST /playlist/<playlist_name>/add`
### `GET /playlist/<playlist_name>/add`

Resolves Spotify objects into tracks and appends them to the named playlist.

For `POST`, you can pass a JSON body with the equivalent structure instead of query params.

POST JSON Body:

```json
{
  "tracks": ["3n3Ppam7vgaVa1iaRUc9Lp"],
  "albums": ["2noRn2Aes5aoNVsU6iWThc"],
  "artists": [],
  "playlists": []
}
```

Accepted query params (for `GET`):

- `tracks[]`
- `albums[]`
- `artists[]`
- `playlists[]`

Example (for `GET`):

```text
GET /playlist/mix/add?tracks[]=3n3Ppam7vgaVa1iaRUc9Lp&albums[]=2noRn2Aes5aoNVsU6iWThc
```

Success response:

```json
{
  "status": "success",
  "added_tracks": 14,
  "total_tracks": 52
}
```

### `POST /playlist/<playlist_name>/where`
### `DELETE /playlist/<playlist_name>/where`

Batch-removes tracks from the named playlist.

For `POST`, you can pass a JSON body with the equivalent structure instead of query params.

POST JSON Body:

```json
{
  "tracks": ["..."],
  "albums": ["..."],
  "artists": [],
  "playlists": []
}
```

Accepted query params (for `DELETE`):

- `tracks[]`
- `albums[]`
- `artists[]`
- `playlists[]`

Success response:

```json
{ "status": "success" }
```

### `DELETE /playlist/<playlist_name>`

Deletes the named playlist file from disk.

If the deleted playlist was active, the server falls back to the default playlist in memory.

Success response:

```json
{ "status": "success" }
```

Saved playlists are always stored under `$XDG_DATA_HOME/spotifm/playlists`
(normally `~/.local/share/spotifm/playlists`). If `default.json` exists there,
it is loaded on startup.

## Lyrics

### `GET /lyrics`

This is the REST lyrics-state endpoint. It is not the WebSocket endpoint.

It returns the cached lyrics state for the current track, which is one of:

- `Lyrics`
- `NoLyrics`
- `Idle`

Live example:

```json
{ "type": "NoLyrics" }
```

Example `Lyrics` payload:

```json
{
  "type": "Lyrics",
  "track_id": "3n3Ppam7vgaVa1iaRUc9Lp",
  "background": 123456,
  "text_color": 16777215,
  "highlight_color": 65280,
  "lines": [
    { "time_ms": 0, "text": "..." },
    { "time_ms": 15420, "text": "..." }
  ]
}
```

## WebSocket

### `GET /ws`

This is the current WebSocket endpoint. `/lyrics` is no longer the socket endpoint.

Optional query parameters:

- `sid`: per-player stream session identifier. When it matches the `sid` used on the audio stream URL, `Position` messages are adjusted to the latest Ogg granule observed for that listener's audio stream.

On connect, the server sends:

1. the current cached lyrics state, or `{"type":"Idle"}`
2. the current `Position` message if a track is already playing
3. the current `NowPlaying` snapshot

Client requests are JSON messages with an `action` field:

```json
{ "action": "get_now_playing" }
```

Supported actions:

- `get_now_playing`
- `get_next`
- `get_playlist`

Outgoing messages are internally tagged with a top-level `"type"` field.

Live initial messages from `/ws`:

```json
{ "type": "NoLyrics" }
```

```json
{ "type": "Position", "position_ms": 47066771 }
```

```json
{
  "type": "NowPlaying",
  "status": "playing",
  "track_id": "3Mv6pY1vvRbVBjuccDjin1",
  "track_name": "Beatnik On The Ship",
  "artists": ["YUZO KOSHIRO"],
  "artist_ids": ["6Tvw2YNOLrcxATPGnFVTzz"],
  "album_id": "22mDiQ83M2QjLM9KHsHQ9z",
  "album_name": "Streets of Rage: Perfect Soundtrack",
  "track_duration_ms": 270093,
  "position_ms": null,
  "listeners": 0,
  "active_playlist": "streetsofrage",
  "cover_url": "https://i.scdn.co/image/ab67616d0000b2737a14c44b2aa2b6f5a85664db"
}
```

`get_next` returns the same flattened playback shape as `NowPlaying`, but tagged as `Next`:

```json
{
  "type": "Next",
  "status": "next",
  "track_id": "3Mv6pY1vvRbVBjuccDjin1",
  "track_name": "Beatnik On The Ship",
  "artists": ["YUZO KOSHIRO"],
  "artist_ids": ["6Tvw2YNOLrcxATPGnFVTzz"],
  "album_id": "22mDiQ83M2QjLM9KHsHQ9z",
  "album_name": "Streets of Rage: Perfect Soundtrack",
  "position_ms": null,
  "listeners": 0,
  "active_playlist": "streetsofrage",
  "cover_url": "https://i.scdn.co/image/ab67616d0000b2737a14c44b2aa2b6f5a85664db"
}
```

If there is no next track, the socket sends:

```json
{ "type": "Next" }
```

`get_playlist` returns the `ActivePlaylistResponse` shape:

```json
{
  "type": "Playlist",
  "name": "default",
  "tracks": [
    {
      "track_id": "1GbtB4zTqAsyfZEsm1RZfx",
      "track_name": "Blinding Lights",
      "artists": ["The Weeknd"],
      "queue_idx": null,
      "artist_ids": ["1XyoAE8jLE1Z1eUzyUsj7C"],
      "album_id": "4yP0hdKOZPNshxUOjY0cZj",
      "album_name": "After Hours",
      "cover_url": "https://i.scdn.co/image/ab67616d0000b2730d..."
    }
  ]
}
```

## Player and stream

### `GET <player_endpoint>`

Default: `GET /`

Serves the web player HTML. The configured `player` setting and `--player` flag
accept a local directory or a direct path to `index.html`.

### `GET <player_endpoint>/minimal`

Default: `GET /minimal`

Serves a minimal reference player from `minimal.html` next to the configured
player path. It includes basic playback, now-playing metadata, progress, album
art, and synced lyrics without the visualizer or settings UI.

### `GET <player_endpoint>/<path>`

Serves additional files from the configured player path. For directory paths,
`<path>` is resolved under that directory; for a direct `index.html` path, it is
resolved next to that file.

### `GET <player_endpoint>/spotifm-player-sw.js`

Default: `GET /spotifm-player-sw.js`

Serves the web player's Service Worker. The player uses it to tee the live audio response in the browser and parse Ogg granules from the same stream that feeds the `<audio>` element.

### `GET <player_endpoint>/spotifm-audio-worklet.js`

Default: `GET /spotifm-audio-worklet.js`

Serves the web player's audio worklet script from the configured player path.

### `GET <player_endpoint>/player-assets/<path>`

Default: `GET /player-assets/<path>`

Serves additional files from the configured player path. This is equivalent to
`GET <player_endpoint>/<path>` and is kept for player assets that use the
existing `player-assets/...` URL layout.

### `GET <stream_endpoint>`

Default: `GET /listen`

Serves the live audio stream.

Ogg responses include `X-Spotifm-Ogg-Granule-Rate`. Consumers that inspect
Ogg granule positions must divide by this value; for example, Opus uses 48000
while the native Vorbis stream normally uses 44100.

The response content type is derived from native passthrough or the selected GStreamer pipeline:

- MP3 pipelines: `audio/mpeg`
- Ogg pipelines: `audio/ogg`
- WebM pipelines: `audio/webm`
- WAV pipelines: `audio/wav`
- AAC/MP4 pipelines: `audio/aac`

## Authentication & Privileges

When API keys are configured in `config.toml`, access to restricted endpoints requires supplying a valid API key.

### Authentication Methods

An API key can be supplied using one of the following:
1. `X-Api-Key` HTTP Header.
   ```text
   X-Api-Key: some_key
   ```
2. `Authorization: Bearer <key>` HTTP Header.
   ```text
   Authorization: Bearer some_key
   ```
3. `api_key=<key>` query parameter.
   ```text
   GET /api/privs?api_key=some_key
   ```

---

### `GET /api/privs`

Returns the active authentication status and the list of permitted privilege scopes for the provided key.

#### Response shape

```json
{
  "auth_enabled": true,
  "authenticated": true,
  "permissions": ["play", "queue", "search"]
}
```

Notes:
- If API authentication is disabled (i.e., no API keys are specified in `config.toml`), the response will be:
  ```json
  {
    "auth_enabled": false,
    "authenticated": true,
    "permissions": ["*"]
  }
  ```
- If an invalid key is provided or the key is missing when authentication is enabled, the response will be:
  ```json
  {
    "auth_enabled": true,
    "authenticated": false,
    "permissions": []
  }
  ```
