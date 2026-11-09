# spotifm (3.3.1)


![spotifm](assets/spotifm.avif)



***spotifm*** is a personal web radio streamer powered by your spotify library

### tldr? check out the demo

<a href="http://waveplate.github.io/spotifm/demo">
  <img src="https://img.shields.io/badge/demo-try_the_spotifm_web_player-green?style=for-the-badge" height="40" alt="Live Demo">
</a>



> [!NOTE]
> a spotify premium subscription is required to use ***spotifm***

## features

* **standalone web radio**
   - no longer requiring co-deployment with icecast and ices
* **flexible audio pipeline**
   - user-definable gstreamer pipelines for encoding, muxing, audio processing, buffering, and output behaviour
* **timed lyrics**
   - retrieves timed lyrics from spotify when possible, with LRCLIB as fallback
* **rest and websocket apis**
   - playback control, search, playlist management, lyrics, telemetry, authorization & scopes
* **cool web player**
   - aesthetically pleasing web player with milkdrop visualizer + realistic NTSC & VHS webgl shaders, live telemetry, song search and playback control. highly configurable
* **reference player**
   - [`minimal reference implementation`](player/minimal.html) of simultaneous playback and WASM-based ogg granule extraction for synchronized lyrics
* **bots**
   - dj bots for [`irc`](bots/irc/) and [`discord`](bots/discord/)

## quick start

### 1) install ***spotifm***

#### one-liner

> [!NOTE]
> ##### ***`curl -fsSL https://raw.githubusercontent.com/waveplate/spotifm/v3.3.1/install.sh | bash -s -- --version 3.3.1`***


this will install:

- `/usr/local/bin/spotifm` (static musl binary)
- `$XDG_DATA_HOME/spotifm/player` or `/usr/local/share/spotifm/player` (web player files)

once installed, start with

`spotifm`

#### other options

<details>

<summary><b><u>docker</u></b></summary>

```
docker run -d \
  -p 3333:3333 \
  -p 3443:3443 \
  -v ${HOME}/.local/share/spotifm:/root/.local/share/spotifm \
  --name spotifm \
  waveplate/spotifm:3.3.1
```

or with docker-compose:

```
git clone https://github.com/waveplate/spotifm
cd spotifm
SPOTIFM_IMAGE=waveplate/spotifm:3.3.1 docker compose up -d spotifm
```

</details>

<details>

<summary><b><u>run without docker</u></b></summary>

see the [build guide](docs/building.md), then start with

`./target/release/spotifm`

</details>

### 2) visit `/oauth` endpoint

visit the `/oauth` endpoint, e.g., http://127.0.0.1:3333/oauth

this endpoint will automatically redirect you to spotify's oauth authorization portal and generally guide you through the authorization process

***why does optional Web API authorization say ncspot?*** -- the commented `rspotify_client_id` example borrows ncspot's app ID. Web API authorization is disabled by default; playback uses `client_id`, and search and metadata use the catalogue fallback. alternatively, see *[linking your own app](docs/configuration.md#linking-your-own-app)*

### 3) listen to some music

> [!important]
> the web player uses a service worker and an audio worklet to play and inspect the audio stream so that lyrics and visualizations remain synchronized
>
> browsers restrict these features to secure contexts: HTTPS, or HTTP on a loopback address such as `127.0.0.1`. this is why ***spotifm*** starts an HTTPS listener in addition to HTTP
>
> in addition to this, certain capabilities like autoplaying audio (without explicit interaction from the user) requires HTTPS regardless of whether it's running locally or not
> 
> **tl;dr:** *just use HTTPS*

#### using the web player

- if running locally, http://127.0.0.1:3333
- if running on a remote server, https://server.ip.or.domain:3443

#### using the /listen endpoint

- http://127.0.0.1:3333/listen

#### play a track

- click the search icon in the top right corner and play some music !
- or, use the API: http://127.0.0.1:3333/play/track?q=dr+worm+they+might+be+giants

---

## documentation

- [building ***spotifm***](docs/building.md)
- [configuration and usage](docs/configuration.md)
- [HTTPS and nginx](docs/https.md)
- [API reference](docs/API.md)
- [interactive OpenAPI documentation](https://waveplate.github.io/spotifm)
- [documentation index](docs/README.md)

---
