# spotify-fm (alpha-1.0.4)

spotify-fm streams your spotify music over the internet using icecast2 and spawns a rest api

## quick start
### 1) configuration
edit `config.env.example` and rename it to `config.env` 


`SPOTIFY_USER` is your email

`SPOTIFY_PASS` is your password

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
#### `GET /np`
#### `GET /skip`
#### `GET /queue/<TRACK-ID>`
#### `GET /play/<TRACK-ID>`
all return (example):
```
{
    "id": "6bu8npt0GdVeESCM7K4The",
    "rid": 1676115018281,
    "track": "Speak Up",
    "artists": [
        "Freddie Dredd"
    ]
}
``` 
or `{ "error": "<error msg>"}`
### `GET /search/<TRACK|ARTIST|ALBUM|PLAYLIST>/<LIMIT>?q=<QUERY>`
returns (example):
```
[
    {
        "album": { ... },
        "artists": [ ... ],
        "available_markets": [ ... ],
        "disc_number": 1,
        "duration_ms": 122331,
        "explicit": true,
        "external_ids": { ... },
        "external_urls": { ... },
        "href": "https://api.spotify.com/v1/tracks/2nzjXDv6OuRHrHKhfhfB98",
        "id": "2nzjXDv6OuRHrHKhfhfB98",
        "is_local": false,
        "name": "Low Key",
        "popularity": 62,
        "preview_url": "https://p.scdn.co/mp3-preview/c4008f1cb619949d448818891c5839404a0d53ad?cid=65b708073fc0480ea92a077233ca87bd",
        "track_number": 3
    },

    ...
]
```
or `{ "error": "<error msg>"}`
