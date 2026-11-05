# configuration and usage

[documentation index](README.md) · [project readme](../README.md)

## linking your own app

you can create an application in the spotify developer dashboard to use a public-facing `redirect_uri`. the configured URI must match the dashboard entry.

1. go to the [spotify developer dashboard](https://developer.spotify.com/dashboard)
2. create a new application
3. add an exact redirect URI such as `https://radio.example:3443/login`
4. enable the **Web API**
5. copy the client ID

```sh
./target/release/spotifm --redirect-uri https://radio.example:3443/login --client-id a1b2c3d4e5f6
```

### Playback and Web API client IDs

Spotifm follows [ncspot's separate authentication flows](https://github.com/hrkfdn/ncspot/blob/47c12502908a952f03e7c9f8224eb28d10c0ca07/src/authentication.rs):

- Playback uses librespot's default client ID, `65b708073fc0480ea92a077233ca87bd`
  on desktop, and reusable `credentials.json` credentials.
- Search and metadata first use the configured Web API client ID, by default
  `d420a117a32841c2b3474932e49fb54b`, and `rspotify_token.json`.

`--client-id` changes the Web API app. Its refresh token must have been issued
for that app. If catalogue requests fail, Spotifm can use anonymous web-player
search and the existing playback session for metadata. See
[anonymous search fallback](anonymous-search.md) for behavior and diagnostics.

When Spotifm's Web API cache is absent, automatic import may copy ncspot's
`rspotify_token.json`. A token without known required scopes is rejected and
requires a fresh Web API authorization. Startup logs identify a cached token,
a refreshed cached token, or a newly authorized token. `--skip-import` disables
automatic credential import.

Spotify documents [rate limits per application](https://developer.spotify.com/documentation/web-api/concepts/rate-limits).
Borrowing ncspot's Web API ID means sharing that app's quota. Catalogue requests
automatically try the backup after `429` and use it during the supplied
`Retry-After` interval. If anonymous search also returns `429`, that error and
wait interval are returned to the caller. Changing access tokens is not a
documented way to reset an application-wide rate limit.

## files and precedence

| file/dir | primary | secondary | tertiary |
| ---- | ---- | --- | --- |
| `config.toml` | `--conf FILE` | `./config.toml` | `$XDG_CONFIG_HOME/spotifm/config.toml` |
| `playlists/` | `$XDG_DATA_HOME/spotifm/playlists/` | `~/.local/share/spotifm/playlists/` when `XDG_DATA_HOME` is unset | -- |
| `player/` | `--player PATH` or `player = "..."` | `./player` | `$XDG_DATA_HOME/spotifm/player` (`~/.local/share/spotifm/player` when unset), `$XDG_DATA_DIRS/spotifm/player` |
| `credentials.json` | `$XDG_CACHE_HOME/spotifm/` | `$XDG_CACHE_HOME/librespot/` | `$XDG_CACHE_HOME/*/librespot/` |
| `rspotify_token.json` | `$XDG_CACHE_HOME/spotifm/` | -- | -- |
| `cert.pem` and `key.pem` | `tls_cert` and `tls_key`, or `--tls-cert` and `--tls-key` | `$XDG_CACHE_HOME/spotifm/tls/` | -- |
| `spotifm.log` | `$XDG_CACHE_HOME/spotifm/` | `~/.cache/spotifm/` when `XDG_CACHE_HOME` is unset | `.spotifm_cache/` fallback |

## configuration options and command-line flags

| toml parameter | cli flag | short | default value | description |
| :--- | :--- | :--- | :--- | :--- |
| *(none)* | `--conf FILE` | `-c` | `./config.toml` if present, otherwise `$XDG_CONFIG_HOME/spotifm/config.toml` | config file path |
| `api_ip` | `--api-ip` | `-s` | `0.0.0.0` | address for both HTTP and HTTPS servers |
| `api_port` | `--api-port` | `-p` | `3333` | HTTP player/API port |
| `cors_origins` | `--cors-origins ORIGIN` | *(none)* | `["*"]` | allowed browser origins; comma-separated or repeated CLI values replace the config list |
| `tls_port` | `--tls-port` | *(none)* | `3443` | HTTPS player/API port |
| `tls_cert` | `--tls-cert FILE` | *(none)* | generated certificate | PEM certificate or certificate-chain file; requires `tls_key` |
| `tls_key` | `--tls-key FILE` | *(none)* | generated private key | PEM private-key file; requires `tls_cert` |
| `client_id` | `--client-id` | `-d` | `d420a117a32841c2b3474932e49fb54b` | spotify client ID |
| `redirect_uri` | `--redirect-uri` | `-r` | `http://127.0.0.1:3333/login` | registered redirect URI for a custom Web API client; bundled loopback clients use `api_port` |
| `oauth_ip` | `--oauth-ip` | `-S` | `0.0.0.0` | oauth listener address |
| `oauth_port` | `--oauth-port` | `-P` | redirect URI port | oauth listener port override |
| `no_browser` | `--no-browser [BOOL]` | `-x` | `false` | prevent opening the authorization page |
| `skip_import` | `--skip-import [BOOL]` | `-X` | `false` | skip importing ncspot/librespot credentials |
| `url_file` | `--url-file` | `-u` | `none` | authorization URL output file |
| `bitrate` | `--bitrate` | `-b` | `320` | spotify source bitrate in kbps (`96`, `160`, or `320`) and `{bitrate}` pipeline value |
| `queue_size` | `--queue-size` | `-Q` | `32` | broadcast buffer capacity |
| `max_buffers` | `--max-buffers` | `-M` | `10` | gstreamer appsink buffer limit and `{max_buffers}` pipeline value |
| `silence_interval` | `--silence-interval` | `-I` | `40` | milliseconds between idle silence chunks |
| `lyrics_interval` | `--lyrics-interval` | `-L` | `250` | milliseconds between lyrics websocket updates |
| `capacity` | `--capacity` | `-C` | `1024` | lyrics websocket broadcast capacity |
| `pipeline` | `--pipeline` | `-g` | `passthrough` | `passthrough`, `opus`, `vorbis`, `mp3`, or a custom pipeline name |
| `stream_endpoint` | `--stream-endpoint` | `-E` | `/listen` | audio-stream path |
| `player_endpoint` | `--player-endpoint` | `-e` | `/` | web-player path |
| `player` | `--player PATH` | `-w` | `./player` if present, otherwise `$XDG_DATA_HOME/spotifm/player` | local player directory or `index.html` path |
| `gst_pipelines.<name>` | *(config only)* | *(none)* | built-in `mp3`, `opus`, and `vorbis` pipelines | custom or overridden gstreamer pipeline template |
| `[[api_key]]` | *(config only)* | *(none)* | none | API key with `key` and a `scope` array |

## cross-origin requests (CORS)

HTTP and HTTPS responses allow any browser origin by default, including API
errors. Preflight `OPTIONS` requests support `GET`, `HEAD`, `POST`, and `DELETE`
with `Authorization`, `X-Api-Key`, `Content-Type`, and `Range` request headers.
Browsers can read `Retry-After`, `X-Spotify-Source`, and
`X-Spotifm-Ogg-Granule-Rate` response headers.

To restrict origins, set a top-level config value:

```toml
cors_origins = ["https://player.example", "http://localhost:5173"]
```

CLI values replace the entire config list:

```sh
spotifm --cors-origins 'https://player.example,http://localhost:5173'
spotifm --cors-origins https://player.example --cors-origins http://localhost:5173
spotifm --cors-origins '*'
```

Origins contain a scheme, host, and optional port. `cors_origins = []` rejects
all requests carrying an `Origin` header; requests without one still work.
CORS does not enable cookie credentials. API keys can be sent using
`Authorization: Bearer ...` or `X-Api-Key`.

## custom gstreamer pipelines

spotifm uses `pipeline = "passthrough"` by default. select `opus`, `vorbis`, `mp3`, or a custom pipeline name to use gstreamer instead.

custom pipelines are defined under `[gst_pipelines]` in `config.toml` and must:

1. contain a raw pcm `appsrc` named `src`
2. end with an `appsink` named `sink`

the available template values are `{bitrate}` and `{max_buffers}`.

For low-latency Ogg output, keep `oggmux` page delay below the lyric update
interval. The built-in Opus and Vorbis pipelines use
`max-delay=20000000 max-page-delay=20000000` (20 ms).
