# configuration and usage

[documentation index](README.md) · [project readme](../README.md)

## linking your own app

you can opt into Web API access with an application in the spotify developer dashboard. `rspotify_redirect_uri` must match the dashboard entry.

1. go to the [spotify developer dashboard](https://developer.spotify.com/dashboard)
2. create a new application
3. add an exact redirect URI such as `https://radio.example:3443/login`
4. enable the **Web API**
5. copy the client ID

```sh
./target/release/spotifm --rspotify-redirect-uri https://radio.example:3443/login --rspotify-client-id a1b2c3d4e5f6
```

### Playback and Web API client IDs

Playback and optional Web API access use separate authentication flows:

- Playback uses `client_id`, defaulting to `65b708073fc0480ea92a077233ca87bd`,
  with `redirect_uri = "http://127.0.0.1:8898/login"` and reusable
  `credentials.json` credentials. The same ID is used for playback OAuth and
  the librespot session, including when no configuration file is available.
- Without `rspotify_client_id`, search uses anonymous web-player search and
  metadata uses the playback session directly. Spotifm skips Web API OAuth,
  token import, cache loading, and token refresh.
- Setting `rspotify_client_id` enables Web API access with its own
  `rspotify_redirect_uri` (default `http://127.0.0.1:8899/ncspot_login`) and
  `rspotify_token.json`. Catalogue failures still use the fallback.

The shipped and automatically generated configs leave these optional settings
commented out:

```toml
client_id = "65b708073fc0480ea92a077233ca87bd"
redirect_uri = "http://127.0.0.1:8898/login"
# rspotify_client_id = "d420a117a32841c2b3474932e49fb54b"
# rspotify_redirect_uri = "http://127.0.0.1:8899/ncspot_login"
```

`--client-id` changes the playback app. `--rspotify-client-id` and
`--rspotify-redirect-uri` override the optional Web API settings independently.
Existing configs using `playback_client_id` should move that value to `client_id`
and move their old Web API ID to `rspotify_client_id` if Web API access is wanted.
Web API refresh tokens must have been issued for the configured Web API app.
See [anonymous search fallback](anonymous-search.md) for behavior and diagnostics.

When the ncspot Web API ID is explicitly enabled and its cache is absent, automatic import may copy ncspot's
`rspotify_token.json`. A token without known required scopes is rejected and
requires a fresh Web API authorization. Startup logs identify a cached token,
a refreshed cached token, or a newly authorized token. `--skip-import` disables
automatic credential import.

After OAuth succeeds, the browser returns to `/oauth/complete` on the main
server using the hostname of the incoming callback request. Hostnames other
than `127.0.0.1` use HTTPS on the configured `tls_port` (3443 by default),
including callbacks through a proxy. Local `127.0.0.1` callbacks can use HTTP.
That page waits for startup (and any enabled second authorization) to finish,
then automatically opens the configured web player. Visiting `/oauth` after
startup applies the same HTTPS rule when opening the player.
In Docker, publish port 8899 as well as 8898 when enabling the default Web API
callback.

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
| `client_id` | `--client-id` | `-d` | `65b708073fc0480ea92a077233ca87bd` | client ID for playback OAuth and the playback session |
| `rspotify_client_id` | `--rspotify-client-id` | *(none)* | disabled | optional Web API app; omit to use the catalogue fallback directly |
| `redirect_uri` | `--redirect-uri` | `-r` | `http://127.0.0.1:8898/login` | playback OAuth redirect URI |
| `rspotify_redirect_uri` | `--rspotify-redirect-uri` | *(none)* | `http://127.0.0.1:8899/ncspot_login` | Web API OAuth redirect URI, used only when `rspotify_client_id` is set |
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
