# spotifm (2.0.0)

spotifm streams your spotify music over the internet using icecast2 and spawns a rest api

> note: spotifm only works with spotify premium accounts

there is an included:
* irc bot which can be used to control the radio through irc commands
* discord bot which streams the radio to a voice channel, and can be controlled through text channels

the commands for both the irc and discord bot:
* `!np` `!prev` `!next` - get the now playing, previous, and next track in the playlist
* `!play <query>` - play this song on the stream immediately
* `!queue <query>` - queue this song to be played next
* `!search <query>` - get top 5 search results for your query
* `!shuffle` - shuffle the playlist

## quick start
### 1) configuration
edit `config.json.example` and rename it to `config.json` 

```
    "user": "your@email.com",
    "pass": "yourpass1",
    "uris": [
        "spotify:playlist:2WvtFSAkmcABdm3iAvYwXk"
    ],
    "elevenlabs": {
        "key": "",
        "voice": ""
    }
```

`user` is your email

`pass` is your password

`uris` is a list of spotify URIs (track, album or playlist) to play once started (`spotify:track:<ID>` or `spotify:album:<ID>` or `spotify:playlist:<ID>`)

optionally:

`elevenlabs` contains your elevenlabs API key, and the voice ID of the voice you want to use for TTS

### 2) track announcments and bumpers (optional)

spotifm can announce the name of the song before it plays, as well as periodically play radio station bumpers of your choosing, configured as follows:

```
    "announce": {
        "song": {
            "enable": false,
            "espeak": {
                "gap": 10,
                "speed": 150,
                "pitch": 50,
                "voice": "en-us",
                "amplitude": 100
            }
        },
        "bumper": {
            "enable": false,
            "freq": 20,
            "tags": [
                "you are listening to my radio"
            ],
            "espeak": {
                "gap": 10,
                "speed": 120,
                "pitch": 50,
                "voice": "en-us",
                "amplitude": 100
            }
        }
    }
```

they are disabled by default

`freq` is how often to play a bumper

see `espeak` manual for description of `gap`, `speed`, `pitch`, `voice` and `amplitude`

### 3) build spotifm

`docker compose run builder`

> note: depending on your docker version, you may need to use `docker-compose` instead of `docker compose`

### 4) deploy spotifm
`docker compose up -d --force-recreate streamer`

> icecast2 will become available on port `8000`, listen to your radio at `http://<your-ip-address>:8000/listen`

> spotifm will spawn a rest api on port `9090`, issue api calls at `http://<your-ip-address>:9090/...`

### 5) irc bot (optional)
make sure to edit `ircbot.json.example` and rename it to `ircbot.json`, then

`docker compose up -d ircbot`

### 6) discord bot (optional)
make sure to edit `discordbot.json.example` and rename it to `discordbot.json`, then

`docker compose up -d discordbot`

## rest api endpoints
### `GET /np`
### `GET /prev`
### `GET /next`
### `GET /skip`
### `GET /queue/<TRACK-ID>`
### `GET /play/<TRACK-ID>`
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
### `GET /playlist`
### `GET /shuffle`
returns (example):
```
[
    {
        "id": "6bu8npt0GdVeESCM7K4The",
        "rid": 1676118353658,
        "track": "Speak Up",
        "artists": [
            "Freddie Dredd"
        ]
    },

    ...
]
```
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

### `POST /announce/bumper`
takes post fields `enable`, `tag`, `freq`, `speed`, `amplitude`, `pitch`, `gap`, `voice` and updates the running instance of your config

if `tag` is supplied, it is appended to your current tags

### `DELETE /announce/bumper/tags`
clears out your tags

### `GET /announce/bumper`
all above return (example):
```
{
    "idx": 1,
    "enable": true,
    "tags": [
        ...
    ],
    "freq": 5,
    "espeak": {
        "speed": 160,
        "amplitude": 60,
        "pitch": 1,
        "gap": 5,
        "voice": "en-us"
    }
}
```

### `POST /announce/song`
takes post fields `enable`, `speed`, `amplitude`, `pitch`, `gap`, `voice` and updates running instance of your config

### `GET /announce/song`
all above return (example):
```
{
    "enable": false,
    "espeak": {
        "speed": 170,
        "amplitude": 100,
        "pitch": 50,
        "gap": 10,
        "voice": "en-us"
    }
}
```

### `GET /mixer/music/fade?start=<START>&end=<END>&duration=<DURATION>`
`START` is the volume percentage to start the fade at, e.g. `100`

`END` is the volume percentage to end the fade at, e.g. `50`

`DURATION` is the duration, in milliseconds, over which to conduct the fade, e.g. `3000`

it returns:
```
{
    "done": true
}
```

### `GET /mixer/announce/volume?vol=<VOLUME>`
`VOLUME` is the volume to set the announcements to, examples of acceptable values are

`120%` - will set the volume to 120%

`+10%` - this will increase the volume by 10%

`140` - this will set the volume to 140dB

it returns:
```
{
    "volume": <VOLUME>
}
```

### `GET /espeak?text=<TEXT>`
### `GET /elevenlabs?text=<TEXT>`
this will use espeak, or the elevenlabs API to speak your `TEXT`

it will return:
```
{
    "text": <TEXT>
}
```