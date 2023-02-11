# spotify-fm (alpha-1.0.0)

spotify-fm streams your spotify music over the internet using icecast2 and spawns a rest api

## quick start
---
### 1) configuration
edit `config.env.example` and rename it to `config.env` 
`SPOTIFY_USER` is your email
`SPOTIFY_USER` is your password
`SPOTIFY_URI` is the track, album or playlist to play once started (`spotify:track:<ID>` or `spotify:album:<ID>` or `spotify:playlist:<ID>`)

#### 2) build spotify-fm
##### docker
`docker compose run --rm -u builder -i build-spotify-fm`
##### native
`cargo build --release`

#### 3) deploy spotify-fm
`docker compose up -d --force-recreate streamer`
> icecast2 will become available on port `8000`
> spotify-fm will spawn a rest api on port `9090`

## rest api endpoints
---
#### `GET /np`
returns (example):
```
{
    "track": "Hysteria",
    "artists": [
        "Undagroundmane",
        "Whiteye$"
    ]
}
``` 
or `{ "error": "<error msg>"}`
#### `GET /skip`
returns `{ "skip": true }` or `{ "error": "<error msg>"}`
#### `GET /queue/<TRACK-ID>`
returns `{ "queue": true }` or `{ "error": "<error msg>"}`
#### `GET /play/<TRACK-ID>`
returns `{ "play": true }` or `{ "error": "<error msg>"}`