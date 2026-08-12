#!/usr/bin/env python3
"""Spotifm Discord radio bot.

Features:
  * joins a Discord voice channel and relays the live radio stream
  * reconnects FFmpeg/voice playback when the stream drops
  * keeps one pinned Components V2 now-playing card updated
  * posts a new now-playing card whenever the track changes
  * exposes the existing !np, !skip, !prev, !next, !play, !queue,
    !search, and !shuffle text commands
  * updates the bot's Listening presence

Python dependencies:
    python -m pip install -U "discord.py[voice]>=2.7,<3" aiohttp Pillow

System dependency:
    ffmpeg
"""

from __future__ import annotations

import asyncio
import colorsys
import io
import json
import logging
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Final
from urllib.parse import quote

import aiohttp
import discord

try:
    from PIL import Image
except ImportError:
    Image = None  # type: ignore[assignment]


LOG = logging.getLogger("spotifm-discord-bot")
DEFAULT_CONFIG_PATH: Final = Path("discordbot.json")
DEFAULT_STATE_PATH: Final = Path.home() / ".local/state/spotifm-discord-bot/state.json"
DEFAULT_ACCENT: Final = 0x1DB954


class ConfigurationError(RuntimeError):
    pass


class SpotifmAPIError(RuntimeError):
    def __init__(self, message: str, *, status: int | None = None) -> None:
        super().__init__(message)
        self.status = status


@dataclass(slots=True, frozen=True)
class Config:
    token: str
    text_channel_ids: list[int]
    voice_channel_id: int
    api_base_url: str
    stream_url: str
    player_url: str | None
    poll_interval: float
    pinned_refresh_interval: float
    announcement_silent: bool
    state_path: Path
    configured_pinned_message_id: int | None
    fallback_accent: int
    ffmpeg_executable: str
    ws_url: str | None

    @classmethod
    def load(cls, path: Path) -> "Config":
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError as exc:
            raise ConfigurationError(f"Config file does not exist: {path}") from exc
        except json.JSONDecodeError as exc:
            raise ConfigurationError(f"Invalid JSON in {path}: {exc}") from exc

        if not isinstance(raw, dict):
            raise ConfigurationError("The config root must be a JSON object")

        def first(*names: str, default: Any = None) -> Any:
            for name in names:
                value = raw.get(name)
                if value is not None:
                    return value
            return default

        token = str(first("token", "discord_token", default="")).strip()
        if not token:
            token = os.environ.get("DISCORD_TOKEN", "").strip()
        if not token:
            raise ConfigurationError("Missing Discord token: config key 'token'")

        text_channel_ids = _required_int_list(
            first(
                "text_channel_ids",
                "textChannelIds",
                "channel_ids",
                "channelIds",
                "channel_id",
                "channelId",
                "textChannelId",
                "nowPlayingChannelId",
                "now_playing_channel_id",
                "discord_channel_id",
            ),
            "textChannelIds",
        )
        voice_channel_id = _required_int(
            first("voice_channel_id", "voiceChannelId"),
            "voiceChannelId",
        )

        api_base_url_val = first(
            "api_base_url",
            "api_url",
            "apiBase",
            "spotifm_url",
        )
        if not api_base_url_val:
            raise ConfigurationError(
                "Missing API base URL: config key 'apiBase' or 'api_base_url'"
            )
        api_base_url = str(api_base_url_val).strip().rstrip("/")

        # Compute and assume the rest of the URLs based on the base URI
        stream_url = f"{api_base_url}/listen"
        player_url = f"{api_base_url}/"

        if api_base_url.startswith("https://"):
            ws_url = api_base_url.replace("https://", "wss://", 1) + "/ws"
        elif api_base_url.startswith("http://"):
            ws_url = api_base_url.replace("http://", "ws://", 1) + "/ws"
        else:
            ws_url = f"ws://{api_base_url}/ws"

        poll_interval = _positive_float(
            first("poll_interval", "pollInterval", default=3.0),
            "poll_interval",
        )
        pinned_refresh_interval = _positive_float(
            first(
                "pinned_refresh_interval",
                "pinnedRefreshInterval",
                default=20.0,
            ),
            "pinned_refresh_interval",
        )

        announcement_silent = _parse_bool(
            first("announcement_silent", "announcementSilent", default=False),
            "announcement_silent",
        )

        state_value = first("state_file", "stateFile", default=DEFAULT_STATE_PATH)
        state_path = Path(str(state_value)).expanduser()
        if not state_path.is_absolute():
            # Keep relative paths predictable for systemd services by resolving them
            # beside the config file, rather than against an arbitrary working directory.
            state_path = path.parent / state_path

        pinned_value = first("pinned_message_id", "pinnedMessageId")
        configured_pinned_message_id = (
            _required_int(pinned_value, "pinned_message_id")
            if pinned_value
            else None
        )

        fallback_accent = _parse_colour(
            first(
                "fallback_accent",
                "fallbackAccent",
                "accent_colour",
                default=DEFAULT_ACCENT,
            )
        )

        ffmpeg_executable = str(
            first("ffmpeg_executable", "ffmpegExecutable", default="ffmpeg")
        ).strip()
        if not ffmpeg_executable:
            raise ConfigurationError("'ffmpeg_executable' cannot be empty")

        return cls(
            token=token,
            text_channel_ids=text_channel_ids,
            voice_channel_id=voice_channel_id,
            api_base_url=api_base_url,
            stream_url=stream_url,
            player_url=player_url,
            poll_interval=poll_interval,
            pinned_refresh_interval=pinned_refresh_interval,
            announcement_silent=announcement_silent,
            state_path=state_path,
            configured_pinned_message_id=configured_pinned_message_id,
            fallback_accent=fallback_accent,
            ffmpeg_executable=ffmpeg_executable,
            ws_url=ws_url,
        )


def _required_int(value: Any, name: str) -> int:
    if value is None:
        raise ConfigurationError(f"Missing config key '{name}'")
    try:
        return int(value)
    except (TypeError, ValueError) as exc:
        raise ConfigurationError(f"'{name}' must be an integer") from exc


def _required_int_list(value: Any, name: str) -> list[int]:
    if value is None:
        raise ConfigurationError(f"Missing config key '{name}'")
    if isinstance(value, list):
        result = []
        for index, item in enumerate(value):
            try:
                result.append(int(item))
            except (TypeError, ValueError) as exc:
                raise ConfigurationError(f"Element at index {index} in '{name}' must be an integer") from exc
        return result
    else:
        try:
            return [int(value)]
        except (TypeError, ValueError) as exc:
            raise ConfigurationError(f"'{name}' must be an integer or a list of integers") from exc


def _positive_float(value: Any, name: str) -> float:
    try:
        parsed = float(value)
    except (TypeError, ValueError) as exc:
        raise ConfigurationError(f"'{name}' must be a number") from exc
    if parsed <= 0:
        raise ConfigurationError(f"'{name}' must be greater than zero")
    return parsed


def _parse_bool(value: Any, name: str) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, int) and value in {0, 1}:
        return bool(value)
    text = str(value).strip().lower()
    if text in {"true", "yes", "on", "1"}:
        return True
    if text in {"false", "no", "off", "0"}:
        return False
    raise ConfigurationError(f"'{name}' must be true or false")


def _parse_colour(value: Any) -> int:
    if isinstance(value, int):
        parsed = value
    else:
        text = str(value).strip().lower()
        if text.startswith("#"):
            text = text[1:]
        if text.startswith("0x"):
            text = text[2:]
        try:
            parsed = int(text, 16)
        except ValueError as exc:
            raise ConfigurationError(
                "'fallback_accent' must be an RGB integer or hex string"
            ) from exc

    if not 0 <= parsed <= 0xFFFFFF:
        raise ConfigurationError(
            "'fallback_accent' must be between 0x000000 and 0xFFFFFF"
        )
    return parsed


@dataclass(slots=True)
class PersistentState:
    pinned_message_ids: dict[int, int] = field(default_factory=dict)
    last_announced_track_id: str | None = None
    last_announcement_message_ids: dict[int, int] = field(default_factory=dict)

    @classmethod
    def load(cls, path: Path) -> "PersistentState":
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return cls()
        except (OSError, json.JSONDecodeError) as exc:
            LOG.warning("Could not read state file %s: %s", path, exc)
            return cls()

        if not isinstance(raw, dict):
            return cls()

        pinned_message_ids = {}
        pinned = raw.get("pinned_message_ids")
        if isinstance(pinned, dict):
            for k, v in pinned.items():
                if v is not None:
                    try:
                        pinned_message_ids[int(k)] = int(v)
                    except ValueError:
                        pass
        # Fallback to old single pinned_message_id
        old_pinned = raw.get("pinned_message_id")
        if old_pinned is not None:
            try:
                pinned_message_ids[0] = int(old_pinned)
            except ValueError:
                pass

        last_announcement_message_ids = {}
        last_announcement = raw.get("last_announcement_message_ids")
        if isinstance(last_announcement, dict):
            for k, v in last_announcement.items():
                if v is not None:
                    try:
                        last_announcement_message_ids[int(k)] = int(v)
                    except ValueError:
                        pass
        # Fallback to old single last_announcement_message_id
        old_announcement = raw.get("last_announcement_message_id")
        if old_announcement is not None:
            try:
                last_announcement_message_ids[0] = int(old_announcement)
            except ValueError:
                pass

        return cls(
            pinned_message_ids=pinned_message_ids,
            last_announced_track_id=(
                str(raw["last_announced_track_id"])
                if raw.get("last_announced_track_id")
                else None
            ),
            last_announcement_message_ids=last_announcement_message_ids,
        )

    def save(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        temp_path = path.with_suffix(path.suffix + ".tmp")

        pinned_json = {str(k): v for k, v in self.pinned_message_ids.items()}
        announcement_json = {str(k): v for k, v in self.last_announcement_message_ids.items()}

        temp_path.write_text(
            json.dumps(
                {
                    "pinned_message_ids": pinned_json,
                    "last_announced_track_id": self.last_announced_track_id,
                    "last_announcement_message_ids": announcement_json,
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        temp_path.replace(path)


@dataclass(slots=True, frozen=True)
class NowPlaying:
    status: str
    track_id: str
    track_name: str
    artists: tuple[str, ...]
    album_name: str | None
    track_duration_ms: int | None
    position_ms: int | None
    listeners: int
    active_playlist: str | None
    cover_url: str | None

    @classmethod
    def from_json(cls, data: Any) -> "NowPlaying":
        if not isinstance(data, dict):
            raise SpotifmAPIError("GET /np returned a non-object response")

        artists = _artist_names(data.get("artists"))

        return cls(
            status=str(data.get("status") or "stopped"),
            track_id=str(data.get("track_id") or data.get("id") or ""),
            track_name=str(
                data.get("track_name")
                or data.get("track")
                or data.get("name")
                or "Unknown track"
            ),
            artists=artists,
            album_name=_optional_string(data.get("album_name")),
            track_duration_ms=_optional_int(
                data.get("track_duration_ms", data.get("duration_ms"))
            ),
            position_ms=_optional_int(data.get("position_ms")),
            listeners=max(0, _optional_int(data.get("listeners")) or 0),
            active_playlist=_optional_string(data.get("active_playlist")),
            cover_url=_optional_string(data.get("cover_url")),
        )

    @property
    def is_active(self) -> bool:
        return self.status in {"playing", "paused"} and bool(self.track_id)

    @property
    def artist_text(self) -> str:
        return ", ".join(self.artists) if self.artists else "Unknown artist"

    @property
    def spotify_url(self) -> str | None:
        if not self.track_id:
            return None
        return f"https://open.spotify.com/track/{quote(self.track_id, safe='')}"

    @property
    def display_fingerprint(self) -> tuple[Any, ...]:
        # Position is intentionally omitted. The relative end timestamp progresses
        # client-side, and the periodic refresh still redraws the progress bar.
        return (
            self.status,
            self.track_id,
            self.track_name,
            self.artists,
            self.album_name,
            self.track_duration_ms,
            self.listeners,
            self.active_playlist,
            self.cover_url,
        )


def _optional_string(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _artist_names(value: Any) -> tuple[str, ...]:
    if not isinstance(value, list):
        return ()

    names: list[str] = []
    for artist in value:
        if isinstance(artist, dict):
            name = artist.get("name")
            if name:
                names.append(str(name))
        elif artist:
            names.append(str(artist))
    return tuple(names)


def make_spotify_text(data: Any) -> str:
    if not isinstance(data, dict):
        return "error generating track info"

    track = data.get("track_name") or data.get("track") or data.get("name")
    artists = _artist_names(data.get("artists"))

    if not track or not artists:
        error = data.get("error")
        return str(error) if error else "error generating track info"

    artist_text = ", ".join(artists)
    return f"`{discord.utils.escape_markdown(str(track))}` by **{discord.utils.escape_markdown(artist_text)}**"


class SpotifmClient:
    def __init__(self, base_url: str, session: aiohttp.ClientSession) -> None:
        self.base_url = base_url.rstrip("/")
        self.session = session

    async def request(
        self,
        path: str,
        *,
        params: dict[str, str] | None = None,
        expect_json: bool = True,
    ) -> Any:
        url = f"{self.base_url}/{path.lstrip('/')}"
        try:
            async with self.session.get(url, params=params) as response:
                body = await response.read()
                if response.status < 200 or response.status >= 300:
                    text = body.decode("utf-8", errors="replace")
                    raise SpotifmAPIError(
                        f"GET {path} returned HTTP {response.status}: {text[:300]}",
                        status=response.status,
                    )

                if not expect_json:
                    return None

                try:
                    return json.loads(body)
                except json.JSONDecodeError as exc:
                    raise SpotifmAPIError(
                        f"GET {path} returned invalid JSON"
                    ) from exc
        except asyncio.TimeoutError as exc:
            raise SpotifmAPIError(f"GET {path} timed out") from exc
        except aiohttp.ClientError as exc:
            raise SpotifmAPIError(f"GET {path} failed: {exc}") from exc

    async def now_playing(self) -> NowPlaying:
        return NowPlaying.from_json(await self.request("np"))


class AccentColourCache:
    def __init__(self, session: aiohttp.ClientSession, fallback: int) -> None:
        self.session = session
        self.fallback = fallback
        self._cache: dict[str, int] = {}

    async def get(self, cover_url: str | None) -> int:
        if not cover_url or Image is None:
            return self.fallback
        if cover_url in self._cache:
            return self._cache[cover_url]

        try:
            async with self.session.get(cover_url) as response:
                if response.status != 200:
                    raise RuntimeError(f"HTTP {response.status}")
                data = await response.read()
                if len(data) > 8 * 1024 * 1024:
                    raise RuntimeError("image is larger than 8 MiB")
            colour = await asyncio.to_thread(
                _extract_accent_colour,
                data,
                self.fallback,
            )
        except (aiohttp.ClientError, asyncio.TimeoutError, OSError, RuntimeError) as exc:
            LOG.debug("Could not derive accent from %s: %s", cover_url, exc)
            colour = self.fallback

        self._cache[cover_url] = colour
        if len(self._cache) > 512:
            self._cache.pop(next(iter(self._cache)))
        return colour


def _extract_accent_colour(data: bytes, fallback: int) -> int:
    assert Image is not None
    with Image.open(io.BytesIO(data)) as image:
        image = image.convert("RGB")
        image.thumbnail((64, 64))
        pixels = list(image.getdata())

    weighted: list[tuple[float, tuple[int, int, int]]] = []
    for red, green, blue in pixels:
        _, saturation, value = colorsys.rgb_to_hsv(
            red / 255.0,
            green / 255.0,
            blue / 255.0,
        )
        if saturation < 0.18 or value < 0.16 or value > 0.94:
            continue
        score = saturation * (1.0 - abs(value - 0.58))
        weighted.append((score, (red, green, blue)))

    if not weighted:
        return fallback

    weighted.sort(key=lambda item: item[0], reverse=True)
    selected = weighted[: max(8, len(weighted) // 5)]
    total_weight = sum(score for score, _ in selected)
    if total_weight <= 0:
        return fallback

    red = round(sum(score * rgb[0] for score, rgb in selected) / total_weight)
    green = round(sum(score * rgb[1] for score, rgb in selected) / total_weight)
    blue = round(sum(score * rgb[2] for score, rgb in selected) / total_weight)
    return (red << 16) | (green << 8) | blue


def _escape(text: str) -> str:
    return discord.utils.escape_markdown(text, as_needed=True)


def _format_duration(milliseconds: int | None) -> str:
    if milliseconds is None or milliseconds < 0:
        return "--:--"
    seconds = milliseconds // 1000
    minutes, seconds = divmod(seconds, 60)
    hours, minutes = divmod(minutes, 60)
    if hours:
        return f"{hours}:{minutes:02d}:{seconds:02d}"
    return f"{minutes}:{seconds:02d}"


def _progress_bar(
    position_ms: int | None,
    duration_ms: int | None,
    width: int = 18,
) -> str:
    if duration_ms is None or duration_ms <= 0:
        return "─" * width
    position = max(0, min(position_ms or 0, duration_ms))
    ratio = position / duration_ms
    marker = min(width - 1, max(0, round(ratio * (width - 1))))
    return "━" * marker + "●" + "─" * (width - marker - 1)


def _end_timestamp(track: NowPlaying) -> int | None:
    if (
        track.status != "playing"
        or track.track_duration_ms is None
        or track.position_ms is None
    ):
        return None
    remaining_ms = max(0, track.track_duration_ms - track.position_ms)
    return int(time.time() + remaining_ms / 1000)


def make_now_playing_view(
    track: NowPlaying,
    *,
    accent: int,
    stream_url: str,
    player_url: str | None,
    announcement: bool,
    lyrics_block: str | None = None,
    position_ms: int | None = None,
) -> discord.ui.LayoutView:
    view = discord.ui.LayoutView(timeout=None)

    heading = "NOW PLAYING" if announcement else "LIVE NOW"
    title = _escape(track.track_name)
    artist = _escape(track.artist_text)
    title_line = (
        f"## [{title}]({track.spotify_url})"
        if track.spotify_url
        else f"## {title}"
    )

    details = [f"### {heading}", title_line, f"**{artist}**"]
    if track.album_name:
        details.append(_escape(track.album_name))

    body_items: list[discord.ui.Item[Any]] = []
    if track.cover_url:
        body_items.append(
            discord.ui.Section(
                discord.ui.TextDisplay("\n".join(details)),
                accessory=discord.ui.Thumbnail(
                    track.cover_url,
                    description=f"Cover artwork for {track.track_name}",
                ),
            )
        )
    else:
        body_items.append(discord.ui.TextDisplay("\n".join(details)))

    if lyrics_block:
        body_items.extend(
            [
                discord.ui.Separator(),
                discord.ui.TextDisplay(lyrics_block),
            ]
        )

    end_timestamp = _end_timestamp(track)
    status_parts: list[str] = []
    if track.status == "paused":
        status_parts.append("**Paused**")
    elif end_timestamp is not None:
        status_parts.append(f"ends <t:{end_timestamp}:R>")

    listener_word = "listener" if track.listeners == 1 else "listeners"
    status_parts.append(f"**{track.listeners} {listener_word}**")
    if track.active_playlist:
        status_parts.append(f"playlist: **{_escape(track.active_playlist)}**")

    display_pos_ms = position_ms if position_ms is not None else track.position_ms
    progress = _progress_bar(display_pos_ms, track.track_duration_ms)
    times = (
        f"`{_format_duration(display_pos_ms)} / "
        f"{_format_duration(track.track_duration_ms)}`"
    )
    body_items.extend(
        [
            discord.ui.Separator(),
            discord.ui.TextDisplay(
                f"{progress}\n{times}  •  " + "  •  ".join(status_parts)
            ),
        ]
    )

    buttons: list[discord.ui.Button[Any]] = []
    if stream_url:
        buttons.append(
            discord.ui.Button(
                label="Listen live",
                style=discord.ButtonStyle.link,
                url=stream_url,
            )
        )
    if player_url and player_url != stream_url:
        buttons.append(
            discord.ui.Button(
                label="Open player",
                style=discord.ButtonStyle.link,
                url=player_url,
            )
        )
    if track.spotify_url:
        buttons.append(
            discord.ui.Button(
                label="Spotify",
                style=discord.ButtonStyle.link,
                url=track.spotify_url,
            )
        )
    if buttons:
        body_items.append(discord.ui.ActionRow(*buttons))

    body_items.append(discord.ui.TextDisplay("-# SPOTIFM RADIO"))
    view.add_item(discord.ui.Container(*body_items, accent_colour=accent))
    return view


def make_stopped_view(accent: int) -> discord.ui.LayoutView:
    view = discord.ui.LayoutView(timeout=None)
    view.add_item(
        discord.ui.Container(
            discord.ui.TextDisplay("### SPOTIFM RADIO\n## Nothing is playing"),
            discord.ui.Separator(),
            discord.ui.TextDisplay(
                "The pinned player will update when playback starts."
            ),
            accent_colour=accent,
        )
    )
    return view


import urllib.request
import struct
import queue
import threading

def detect_ogg_granule_rate(page: bytes) -> float | None:
    """Return the codec's Ogg granule clock rate from an identification page."""
    if len(page) < 27:
        return None

    segment_count = page[26]
    body_offset = 27 + segment_count
    if body_offset > len(page):
        return None
    body = page[body_offset:]

    # RFC 7845 defines every Opus granule position in units of 48 kHz,
    # independently of the source or decoder output sample rate.
    if body.startswith(b"OpusHead"):
        return 48000.0

    # A Vorbis identification packet carries its actual sample rate.
    if len(body) >= 16 and body.startswith(b"\x01vorbis"):
        sample_rate = struct.unpack("<I", body[12:16])[0]
        if sample_rate > 0:
            return float(sample_rate)

    return None


class OggStreamReader(io.BufferedIOBase):
    def __init__(self, url: str, bot: SpotifmBot) -> None:
        super().__init__()
        self.url = url
        self.bot = bot
        self.queue = queue.Queue(maxsize=100)  # Buffers up to 100 * 4KB = 400KB of audio (~25 seconds)
        self.read_buffer = b""
        self.granule_rate: float | None = None
        self._closed = False
        self.download_thread = threading.Thread(target=self._download_loop, daemon=True)
        self.download_thread.start()

    def readable(self) -> bool:
        return True

    @property
    def closed(self) -> bool:
        return self._closed

    def _download_loop(self) -> None:
        response = None
        ogg_buffer = b""
        try:
            req = urllib.request.Request(
                self.url,
                headers={"User-Agent": "Spotifm-DiscordBot/2.0"}
            )
            response = urllib.request.urlopen(req, timeout=15)
            advertised_rate = response.headers.get("X-Spotifm-Ogg-Granule-Rate")
            if advertised_rate is not None:
                try:
                    parsed_rate = float(advertised_rate)
                    if parsed_rate > 0:
                        self.granule_rate = parsed_rate
                except ValueError:
                    LOG.warning(
                        "Ignoring invalid Ogg granule rate header: %r",
                        advertised_rate,
                    )
            
            while not self._closed:
                # Read a larger chunk from network for efficient buffering
                chunk = response.read(4096)
                if not chunk:
                    break
                
                # Parse Ogg pages for granules
                ogg_buffer += chunk
                while True:
                    idx = ogg_buffer.find(b"OggS")
                    if idx == -1:
                        if len(ogg_buffer) > 3:
                            ogg_buffer = ogg_buffer[-3:]
                        break
                    ogg_buffer = ogg_buffer[idx:]
                    if len(ogg_buffer) < 27:
                        break
                    page_segments = ogg_buffer[26]
                    header_len = 27 + page_segments
                    if len(ogg_buffer) < header_len:
                        break
                    segment_table = ogg_buffer[27:header_len]
                    data_len = sum(segment_table)
                    total_page_len = header_len + data_len
                    if len(ogg_buffer) < total_page_len:
                        break

                    page = ogg_buffer[:total_page_len]
                    detected_rate = detect_ogg_granule_rate(page)
                    if detected_rate is not None:
                        self.granule_rate = detected_rate

                    lo, hi = struct.unpack("<II", ogg_buffer[6:14])
                    if lo != 0xffffffff or hi != 0xffffffff:
                        granule_pos = lo + hi * 4294967296
                        if granule_pos > 0 and self.granule_rate is not None:
                            self.bot.current_stream_granule_sec = granule_pos / self.granule_rate
                            self.bot.current_stream_granule_time = time.monotonic()
                    ogg_buffer = ogg_buffer[total_page_len:]
                
                # Put the chunk in queue (blocks if queue is full)
                self.queue.put(chunk)
        except Exception as e:
            LOG.error("Error in Ogg stream downloader loop: %s", e)
        finally:
            if response is not None:
                try:
                    response.close()
                except Exception:
                    pass
            # Signal EOF
            self.queue.put(None)

    def read(self, size: int = -1) -> bytes:
        if size <= 0:
            size = 4096

        while len(self.read_buffer) < size:
            try:
                # Block for a short time if queue is empty
                chunk = self.queue.get(timeout=0.1)
                if chunk is None:
                    # Sentinel received (EOF or error)
                    break
                self.read_buffer += chunk
            except queue.Empty:
                if self._closed:
                    break
                continue

        if not self.read_buffer:
            return b""

        chunk_to_return = self.read_buffer[:size]
        self.read_buffer = self.read_buffer[size:]
        return chunk_to_return

    def close(self) -> None:
        self._closed = True
        super().close()


class OggGranuleAudioSource(discord.FFmpegOpusAudio):
    def __init__(self, source: str, bot: SpotifmBot, **kwargs) -> None:
        self.bot = bot
        self.reader = OggStreamReader(source, bot)
        super().__init__(self.reader, pipe=True, **kwargs)

    def cleanup(self) -> None:
        self.reader.close()
        super().cleanup()


class ChannelLyricUpdater:
    def __init__(self, bot: SpotifmBot, channel_id: int) -> None:
        self.bot = bot
        self.channel_id = channel_id
        self.pending_update: tuple[NowPlaying, int, discord.ui.View] | None = None
        self.task: asyncio.Task[None] | None = None
        self.edit_timestamps: list[float] = []

    def request_update(self, track: NowPlaying, accent: int, view: discord.ui.View) -> None:
        self.pending_update = (track, accent, view)
        if self.task is None or self.task.done():
            self.task = asyncio.create_task(
                self._run(),
                name=f"spotifm-lyric-updater-{self.channel_id}",
            )

    def clear_pending(self) -> None:
        self.pending_update = None
        self.edit_timestamps.clear()

    async def _run(self) -> None:
        while self.pending_update is not None:
            now = time.monotonic()
            self.edit_timestamps = [t for t in self.edit_timestamps if now - t < 5.2]

            if len(self.edit_timestamps) >= 5:
                wait_time = (self.edit_timestamps[0] + 5.2) - now
                if wait_time > 0:
                    await asyncio.sleep(wait_time)
                continue

            update_data = self.pending_update
            self.pending_update = None
            if update_data is None:
                break

            track, accent, view = update_data

            if self.bot.last_track is None or track.track_id != self.bot.last_track.track_id:
                break

            channel = self.bot.text_channels.get(self.channel_id)
            if channel is None:
                await self.bot._resolve_text_channels()
                channel = self.bot.text_channels.get(self.channel_id)
            if channel is None:
                break

            message = await self.bot._load_announcement_message(channel)
            if message is not None:
                try:
                    self.edit_timestamps.append(time.monotonic())
                    updated_message = await message.edit(
                        content=None,
                        embed=None,
                        attachments=[],
                        view=view,
                    )
                    self.bot.last_announcement_messages[self.channel_id] = updated_message
                except discord.NotFound:
                    self.bot.last_announcement_messages.pop(self.channel_id, None)
                    self.bot.state.last_announcement_message_ids.pop(self.channel_id, None)
                    self.bot.state.save(self.bot.config.state_path)
                except discord.HTTPException as e:
                    LOG.warning(
                        "Failed to edit announcement message in channel %s: %s",
                        self.channel_id,
                        e,
                    )


class SpotifmBot(discord.Client):
    def __init__(self, config: Config) -> None:
        intents = discord.Intents.none()
        intents.guilds = True
        intents.guild_messages = True
        intents.message_content = True
        intents.voice_states = True

        super().__init__(
            intents=intents,
            allowed_mentions=discord.AllowedMentions.none(),
        )

        self.config = config
        self.state = PersistentState.load(config.state_path)

        # Migrate old single-channel entries if present
        if config.text_channel_ids:
            first_channel_id = config.text_channel_ids[0]
            if 0 in self.state.pinned_message_ids:
                self.state.pinned_message_ids[first_channel_id] = self.state.pinned_message_ids.pop(0)
            if 0 in self.state.last_announcement_message_ids:
                self.state.last_announcement_message_ids[first_channel_id] = self.state.last_announcement_message_ids.pop(0)

            if config.configured_pinned_message_id is not None:
                self.state.pinned_message_ids[first_channel_id] = config.configured_pinned_message_id

        self.http_session: aiohttp.ClientSession | None = None
        self.api: SpotifmClient | None = None
        self.accent_cache: AccentColourCache | None = None
        self.poll_task: asyncio.Task[None] | None = None
        self.voice_restart_task: asyncio.Task[None] | None = None
        self.ws_task: asyncio.Task[None] | None = None
        self.lyrics_sync_task: asyncio.Task[None] | None = None
        self.parse_granule_task: asyncio.Task[None] | None = None
        self.event_loop: asyncio.AbstractEventLoop | None = None

        self.text_channels: dict[int, discord.abc.Messageable] = {}
        self.pinned_messages: dict[int, discord.Message] = {}
        self.last_announcement_messages: dict[int, discord.Message] = {}
        self.channel_updaters: dict[int, ChannelLyricUpdater] = {}
        self.last_displayed_lyric_index: int | None = None
        self.last_pinned_fingerprint: tuple[Any, ...] | None = None
        self.last_pinned_update_monotonic = 0.0

        self.lyrics: list[dict[str, Any]] | None = None
        self.lyrics_track_id: str | None = None
        self.last_position_ms: int = 0
        self.last_position_received_at: float = 0.0
        self.current_lyric_index: int | None = None
        self.lyrics_offset_ms: int = 0
        self.start_granule_sec: float | None = None
        self.stream_origin_sec: float | None = None
        self.current_stream_granule_sec: float | None = None
        self.current_stream_granule_time: float = 0.0
        self.voice_play_start_time: float | None = None
        self.last_track: NowPlaying | None = None
        self.last_accent: int = DEFAULT_ACCENT

        self.voice_lock = asyncio.Lock()
        self.shutting_down = False
        self._ready_logged = False

    async def setup_hook(self) -> None:
        self.event_loop = asyncio.get_running_loop()

        timeout = aiohttp.ClientTimeout(total=20, connect=7, sock_read=15)
        self.http_session = aiohttp.ClientSession(
            timeout=timeout,
            headers={"User-Agent": "Spotifm-DiscordBot/2.0"},
        )
        self.api = SpotifmClient(self.config.api_base_url, self.http_session)
        self.accent_cache = AccentColourCache(
            self.http_session,
            self.config.fallback_accent,
        )
        self.poll_task = asyncio.create_task(
            self._now_playing_loop(),
            name="spotifm-now-playing",
        )
        self.ws_task = asyncio.create_task(
            self._ws_listener_loop(),
            name="spotifm-ws-listener",
        )
        self.lyrics_sync_task = asyncio.create_task(
            self._lyrics_sync_loop(),
            name="spotifm-lyrics-sync",
        )

    async def close(self) -> None:
        self.shutting_down = True

        for updater in list(self.channel_updaters.values()):
            if updater.task is not None:
                updater.task.cancel()
        for updater in list(self.channel_updaters.values()):
            if updater.task is not None:
                try:
                    await updater.task
                except asyncio.CancelledError:
                    pass
        self.channel_updaters.clear()

        if self.poll_task is not None:
            self.poll_task.cancel()
            try:
                await self.poll_task
            except asyncio.CancelledError:
                pass
            self.poll_task = None

        if self.ws_task is not None:
            self.ws_task.cancel()
            try:
                await self.ws_task
            except asyncio.CancelledError:
                pass
            self.ws_task = None

        if self.lyrics_sync_task is not None:
            self.lyrics_sync_task.cancel()
            try:
                await self.lyrics_sync_task
            except asyncio.CancelledError:
                pass
            self.lyrics_sync_task = None

        if self.voice_restart_task is not None:
            self.voice_restart_task.cancel()
            try:
                await self.voice_restart_task
            except asyncio.CancelledError:
                pass
            self.voice_restart_task = None

        for voice_client in list(self.voice_clients):
            try:
                if voice_client.is_connected():
                    await voice_client.disconnect(force=True)
            except Exception:
                LOG.exception("Failed to disconnect voice client")

        if self.http_session is not None and not self.http_session.closed:
            await self.http_session.close()
            self.http_session = None

        await super().close()

    async def on_ready(self) -> None:
        if not self._ready_logged:
            LOG.info(
                "Logged in as %s (%s)",
                self.user,
                self.user.id if self.user else "?",
            )
            self._ready_logged = True

        try:
            await self.connect_to_radio()
        except Exception:
            LOG.exception("Could not connect to the voice channel")

    async def connect_to_radio(self) -> None:
        async with self.voice_lock:
            if self.shutting_down:
                return

            channel = self.get_channel(self.config.voice_channel_id)
            if channel is None:
                channel = await self.fetch_channel(self.config.voice_channel_id)

            if not isinstance(
                channel,
                (discord.VoiceChannel, discord.StageChannel),
            ):
                raise TypeError(
                    f"Channel {self.config.voice_channel_id} is not a voice channel"
                )

            LOG.info(
                "Using voice channel %s in guild %s",
                channel.name,
                channel.guild.name,
            )

            voice_client = discord.utils.get(
                self.voice_clients,
                guild=channel.guild,
            )

            if voice_client is None:
                voice_client = await channel.connect(
                    timeout=20.0,
                    reconnect=True,
                    self_deaf=True,
                )
            elif not voice_client.is_connected():
                await voice_client.disconnect(force=True)
                voice_client = await channel.connect(
                    timeout=20.0,
                    reconnect=True,
                    self_deaf=True,
                )
            elif voice_client.channel.id != channel.id:
                await voice_client.move_to(channel)

            if not voice_client.is_playing():
                self.start_radio_stream(voice_client)

    def start_radio_stream(self, voice_client: discord.VoiceClient) -> None:
        if self.shutting_down:
            return
        if not voice_client.is_connected():
            LOG.warning("Cannot start stream: voice client is disconnected")
            return
        if voice_client.is_playing():
            return

        self.current_stream_granule_sec = None
        self.current_stream_granule_time = 0.0

        source = OggGranuleAudioSource(
            self.config.stream_url,
            self,
            executable=self.config.ffmpeg_executable,
            codec="libvorbis",
            options="-vn -ar 48000 -ac 1 -b:a 96k",
        )

        self.voice_play_start_time = time.monotonic()

        voice_client.play(
            source,
            after=lambda error: self.on_stream_finished(voice_client, error),
        )
        LOG.info("Radio stream started")

    def on_stream_finished(
        self,
        voice_client: discord.VoiceClient,
        error: Exception | None,
    ) -> None:
        if error is not None:
            LOG.error("Radio stream stopped with an error: %s", error)
        else:
            LOG.warning("Radio stream ended")

        if self.shutting_down or self.event_loop is None:
            return

        def schedule_restart() -> None:
            if self.shutting_down:
                return
            if self.voice_restart_task is not None and not self.voice_restart_task.done():
                return
            self.voice_restart_task = asyncio.create_task(
                self.restart_radio_stream(voice_client),
                name="spotifm-voice-restart",
            )

        self.event_loop.call_soon_threadsafe(schedule_restart)

    async def restart_radio_stream(
        self,
        voice_client: discord.VoiceClient,
    ) -> None:
        try:
            await asyncio.sleep(2)
            if self.shutting_down:
                return

            if voice_client.is_connected():
                if not voice_client.is_playing():
                    LOG.info("Restarting radio stream")
                    self.start_radio_stream(voice_client)
            else:
                await self.connect_to_radio()
        except asyncio.CancelledError:
            raise
        except Exception:
            LOG.exception("Could not restart the radio stream")

    async def _resolve_text_channels(self) -> list[discord.abc.Messageable]:
        resolved = []
        for channel_id in self.config.text_channel_ids:
            if channel_id in self.text_channels:
                resolved.append(self.text_channels[channel_id])
                continue

            try:
                cached = self.get_channel(channel_id)
                channel = (
                    cached
                    if cached is not None
                    else await self.fetch_channel(channel_id)
                )
                if not isinstance(channel, discord.abc.Messageable):
                    LOG.error("Discord channel %s cannot receive messages", channel_id)
                    continue
                self.text_channels[channel_id] = channel
                resolved.append(channel)
            except Exception as e:
                LOG.error("Failed to resolve text channel %s: %s", channel_id, e)
        return resolved

    async def _load_pinned_message(
        self,
        channel: discord.abc.Messageable,
    ) -> discord.Message | None:
        channel_id = getattr(channel, "id", None)
        if channel_id is None:
            return None

        if channel_id in self.pinned_messages:
            return self.pinned_messages[channel_id]

        saved_id = self.state.pinned_message_ids.get(channel_id)
        if saved_id is None:
            return None

        fetch_message = getattr(channel, "fetch_message", None)
        if fetch_message is None:
            return None

        try:
            message = await fetch_message(saved_id)
        except discord.NotFound:
            LOG.warning(
                "Saved pinned message %s in channel %s no longer exists; creating a new one",
                saved_id,
                channel_id,
            )
            self.state.pinned_message_ids.pop(channel_id, None)
            self.state.save(self.config.state_path)
            return None
        except discord.Forbidden as exc:
            LOG.warning(
                "Bot cannot read message history in channel %s: %s",
                channel_id,
                exc,
            )
            return None

        if self.user is not None and message.author.id != self.user.id:
            LOG.warning(
                "Configured pinned message %s in channel %s is not owned by this bot; creating a new one",
                message.id,
                channel_id,
            )
            self.state.pinned_message_ids.pop(channel_id, None)
            self.state.save(self.config.state_path)
            return None

        self.pinned_messages[channel_id] = message
        return message

    async def _create_pinned_message(
        self,
        channel: discord.abc.Messageable,
        view: discord.ui.LayoutView,
    ) -> discord.Message:
        message = await channel.send(view=view)
        try:
            await message.pin(reason="Spotifm persistent now-playing display")
        except discord.Forbidden:
            LOG.error(
                "Created now-playing message %s but cannot pin it in channel %s. "
                "Grant the bot Manage Messages.",
                message.id,
                channel,
            )

        channel_id = getattr(channel, "id")
        self.pinned_messages[channel_id] = message
        self.state.pinned_message_ids[channel_id] = message.id
        self.state.save(self.config.state_path)
        return message

    async def _load_announcement_message(
        self,
        channel: discord.abc.Messageable,
    ) -> discord.Message | None:
        channel_id = getattr(channel, "id", None)
        if channel_id is None:
            return None

        if channel_id in self.last_announcement_messages:
            return self.last_announcement_messages[channel_id]

        saved_id = self.state.last_announcement_message_ids.get(channel_id)
        if saved_id is None:
            return None

        fetch_message = getattr(channel, "fetch_message", None)
        if fetch_message is None:
            return None

        try:
            message = await fetch_message(saved_id)
        except discord.NotFound:
            LOG.warning(
                "Saved announcement message %s in channel %s no longer exists.",
                saved_id,
                channel_id,
            )
            self.state.last_announcement_message_ids.pop(channel_id, None)
            self.state.save(self.config.state_path)
            return None
        except discord.Forbidden as exc:
            LOG.warning(
                "Bot cannot read message history in channel %s: %s",
                channel_id,
                exc,
            )
            return None

        self.last_announcement_messages[channel_id] = message
        return message

    async def _update_announcement_lyrics(
        self,
        track: NowPlaying | None,
        *,
        accent: int,
        force: bool = False,
        force_progress_update: bool = False,
    ) -> None:
        if track is None:
            return

        now = time.monotonic()
        fingerprint = (track.display_fingerprint, self.current_lyric_index)

        # Edit if the fingerprint (track details or active lyric index) has changed.
        if not force and not force_progress_update:
            if fingerprint == self.last_pinned_fingerprint:
                return

        # Get the interpolated position
        position_ms = self.get_current_position_ms()
        if position_ms is None:
            position_ms = track.position_ms

        view = make_now_playing_view(
            track,
            accent=accent,
            stream_url=self.config.stream_url,
            player_url=self.config.player_url,
            announcement=True,
            lyrics_block=self._get_lyrics_block(),
            position_ms=position_ms,
        )

        channels = await self._resolve_text_channels()
        for channel in channels:
            channel_id = getattr(channel, "id", None)
            if channel_id is None:
                continue

            updater = self.channel_updaters.get(channel_id)
            if updater is None:
                updater = ChannelLyricUpdater(self, channel_id)
                self.channel_updaters[channel_id] = updater
            updater.request_update(track, accent, view)

        self.last_pinned_fingerprint = fingerprint
        self.last_pinned_update_monotonic = now
        self.last_displayed_lyric_index = self.current_lyric_index

    async def _announce_track(self, track: NowPlaying, accent: int) -> None:
        view = make_now_playing_view(
            track,
            accent=accent,
            stream_url=self.config.stream_url,
            player_url=self.config.player_url,
            announcement=True,
            lyrics_block=self._get_lyrics_block(),
        )
        channels = await self._resolve_text_channels()
        for channel in channels:
            channel_id = getattr(channel, "id", None)
            if channel_id is None:
                continue

            if channel_id in self.channel_updaters:
                self.channel_updaters[channel_id].clear_pending()

            try:
                message = await channel.send(
                    view=view,
                    silent=self.config.announcement_silent,
                )
                self.last_announcement_messages[channel_id] = message
                self.state.last_announcement_message_ids[channel_id] = message.id
            except Exception as e:
                LOG.error("Failed to announce track in channel %s: %s", channel_id, e)

        self.state.last_announced_track_id = track.track_id
        self.state.save(self.config.state_path)
        LOG.info("Announced: %s - %s", track.artist_text, track.track_name)

    async def _process_now_playing(self, track: NowPlaying) -> None:
        assert self.accent_cache is not None

        if not track.is_active:
            self.last_track = None
            self.last_accent = self.config.fallback_accent
            await self.change_presence(activity=None)
            self.last_announcement_messages.clear()
            self.state.last_announcement_message_ids.clear()
            self.state.save(self.config.state_path)
            return

        accent = await self.accent_cache.get(track.cover_url)
        track_changed = self.state.last_announced_track_id != track.track_id

        self.last_track = track
        self.last_accent = accent

        await self.change_presence(
            activity=discord.Activity(
                type=discord.ActivityType.listening,
                name=f"{track.artist_text} - {track.track_name}"[:128],
            )
        )

        if track_changed:
            await self._announce_track(track, accent)
        else:
            await self._update_announcement_lyrics(track, accent=accent, force=False)

    async def _now_playing_loop(self) -> None:
        await self.wait_until_ready()
        assert self.api is not None

        failure_count = 0
        while not self.is_closed():
            try:
                track = await self.api.now_playing()
                await self._process_now_playing(track)
                failure_count = 0
                delay = self.config.poll_interval
            except asyncio.CancelledError:
                raise
            except (
                SpotifmAPIError,
                discord.HTTPException,
                RuntimeError,
                OSError,
            ) as exc:
                failure_count += 1
                delay = min(
                    max(self.config.poll_interval, 2.0)
                    * (2 ** min(failure_count - 1, 4)),
                    60.0,
                )
                LOG.error(
                    "Now-playing update failed: %s; retrying in %.1fs",
                    exc,
                    delay,
                )

            await asyncio.sleep(delay)

    async def on_message(self, message: discord.Message) -> None:
        if message.author.bot:
            return

        content = message.content.strip()
        if not content.startswith("!"):
            return

        parts = content.split(maxsplit=1)
        command = parts[0][1:].lower()
        text = parts[1].strip() if len(parts) > 1 else ""

        if command not in {
            "np",
            "skip",
            "prev",
            "next",
            "play",
            "queue",
            "search",
            "shuffle",
            "offset",
            "timing",
            "help",
        }:
            return

        assert self.api is not None

        try:
            if command in {"np", "skip", "prev", "next"}:
                response = await self.api.request(command)
                await message.channel.send(make_spotify_text(response))

            elif command in {"play", "queue"} and text:
                results = await self.api.request(
                    "search/track/1",
                    params={"q": text},
                )
                if not isinstance(results, list) or not results:
                    await message.channel.send("no tracks found")
                    return

                first_result = results[0]
                if not isinstance(first_result, dict):
                    await message.channel.send("invalid search result")
                    return

                track_id = first_result.get("track_id") or first_result.get("id")
                if not track_id:
                    await message.channel.send("search result has no track ID")
                    return

                escaped_id = quote(str(track_id), safe="")
                endpoint = (
                    f"play/track/{escaped_id}"
                    if command == "play"
                    else f"queue/track/{escaped_id}"
                )
                response = await self.api.request(endpoint)
                await message.channel.send(make_spotify_text(response))

            elif command == "queue":
                response = await self.api.request("queue")
                if not isinstance(response, list) or not response:
                    await message.channel.send("queue is empty")
                    return

                lines = [
                    f"{index}. {make_spotify_text(track)}"
                    for index, track in enumerate(response[:10], start=1)
                ]
                if len(response) > 10:
                    lines.append(f"...and {len(response) - 10} more")
                await message.channel.send("\n".join(lines))

            elif command == "search" and text:
                results = await self.api.request(
                    "search/track/5",
                    params={"q": text},
                )
                if not isinstance(results, list) or not results:
                    await message.channel.send("no tracks found")
                    return

                lines = [
                    f"{index}. {make_spotify_text(track)}"
                    for index, track in enumerate(results, start=1)
                ]
                await message.channel.send("\n".join(lines))

            elif command == "shuffle":
                try:
                    await self.api.request("playlist/shuffle")
                except SpotifmAPIError as exc:
                    # Compatibility with the endpoint used by the older bot.
                    if exc.status != 404:
                        raise
                    await self.api.request("shuffle", expect_json=False)
                await message.channel.send("*playlist shuffled*")

            elif command == "help":
                help_text = (
                    "**Available Commands:**\n"
                    "• `!np` — Show the currently playing track\n"
                    "• `!play <track>` — Search and play a track immediately\n"
                    "• `!queue [track]` — Queue a track, or show the current queue if no track is specified\n"
                    "• `!search <track>` — Search for a track and list top 5 results\n"
                    "• `!skip` / `!next` — Skip to the next track\n"
                    "• `!prev` — Go back to the previous track\n"
                    "• `!shuffle` — Shuffle the current playlist\n"
                    "• `!offset [ms]` — Get or set the lyrics timing offset in milliseconds (e.g., `!offset -500` makes lyrics appear 500ms earlier)\n"
                    "• `!help` — Show this help message"
                )
                await message.channel.send(help_text)

            elif command in {"offset", "timing"}:
                if not text:
                    await message.channel.send(f"*lyrics offset is currently {self.lyrics_offset_ms}ms*")
                else:
                    try:
                        val = int(text)
                        self.lyrics_offset_ms = val
                        await message.channel.send(f"*lyrics offset set to {val}ms*")
                        await self._update_lyric_index()
                    except ValueError:
                        await message.channel.send("*invalid offset format; please provide a number in milliseconds (e.g., !offset -500)*")

            elif command in {"play", "search"}:
                await message.channel.send(f"usage: !{command} <track>")

        except SpotifmAPIError as exc:
            LOG.exception("Radio API request failed")
            if exc.status is not None:
                await message.channel.send(f"radio API error: HTTP {exc.status}")
            else:
                await message.channel.send("could not contact the radio API")
        except discord.HTTPException:
            LOG.exception("Discord command response failed")
        except Exception:
            LOG.exception("Command failed")
            try:
                await message.channel.send("command failed")
            except discord.HTTPException:
                pass



    def _build_ws_url(self) -> str:
        if self.config.ws_url:
            ws_base = self.config.ws_url.rstrip("/")
            if "/ws" in ws_base:
                return ws_base
            else:
                return f"{ws_base}/ws"
        else:
            base = self.config.api_base_url
            if base.startswith("https://"):
                ws_base = base.replace("https://", "wss://", 1)
            elif base.startswith("http://"):
                ws_base = base.replace("http://", "ws://", 1)
            else:
                ws_base = "ws://" + base
            return f"{ws_base}/ws"

    async def _ws_listener_loop(self) -> None:
        await self.wait_until_ready()
        
        failure_count = 0
        while not self.is_closed() and not self.shutting_down:
            try:
                ws_url = self._build_ws_url()
                LOG.info("Connecting to lyrics WebSocket: %s", ws_url)
                async with self.http_session.ws_connect(ws_url) as ws:
                    failure_count = 0
                    async for msg in ws:
                        if self.shutting_down:
                            break
                        if msg.type == aiohttp.WSMsgType.TEXT:
                            try:
                                data = json.loads(msg.data)
                                await self._handle_ws_message(data)
                            except Exception as e:
                                LOG.error("Failed to handle WebSocket message: %s", e)
                        elif msg.type in (aiohttp.WSMsgType.CLOSED, aiohttp.WSMsgType.ERROR):
                            break
            except asyncio.CancelledError:
                raise
            except Exception as e:
                failure_count += 1
                delay = min(5.0 * (2 ** min(failure_count - 1, 4)), 60.0)
                LOG.error("WebSocket connection failed: %s; retrying in %.1fs", e, delay)
                await asyncio.sleep(delay)

    async def _handle_ws_message(self, data: dict[str, Any]) -> None:
        msg_type = data.get("type")
        if msg_type == "Lyrics":
            lines = data.get("lines") or []
            self.lyrics = sorted(lines, key=lambda x: x.get("time_ms", 0))
            self.lyrics_track_id = data.get("track_id")
            await self._update_lyric_index()
        elif msg_type == "NoLyrics":
            self.lyrics = None
            self.lyrics_track_id = None
            self.current_lyric_index = None
            await self._trigger_pinned_update()
        elif msg_type == "Idle":
            self.lyrics = None
            self.lyrics_track_id = None
            self.current_lyric_index = None
            self.last_position_ms = 0
            self.last_position_received_at = 0.0
            self.start_granule_sec = None
            self.stream_origin_sec = None
            await self._trigger_pinned_update()
        elif msg_type == "Position":
            self.last_position_ms = data.get("position_ms", 0)
            self.last_position_received_at = time.monotonic()
            if "start_granule_sec" in data:
                val = data.get("start_granule_sec")
                self.start_granule_sec = float(val) if val is not None else None
            if "stream_origin_sec" in data:
                val = data.get("stream_origin_sec")
                self.stream_origin_sec = float(val) if val is not None else None
            await self._update_lyric_index()

    async def _lyrics_sync_loop(self) -> None:
        await self.wait_until_ready()
        while not self.is_closed() and not self.shutting_down:
            try:
                if self.last_track is not None and self.last_track.is_active:
                    await self._update_lyric_index()
            except Exception as e:
                LOG.error("Error in lyrics sync loop: %s", e)
            await asyncio.sleep(0.1)

    async def _update_lyric_index(self) -> None:
        lyric_updated = False
        if not self.lyrics:
            if self.current_lyric_index is not None or self.last_displayed_lyric_index is not None:
                self.current_lyric_index = None
                self.last_displayed_lyric_index = None
                await self._trigger_pinned_update()
                lyric_updated = True
        else:
            current_pos_ms = self.get_current_position_ms()
            if current_pos_ms is not None:
                new_idx = self.find_active_lyric_index(current_pos_ms)
                if new_idx != self.last_displayed_lyric_index or new_idx != self.current_lyric_index:
                    self.current_lyric_index = new_idx
                    await self._trigger_pinned_update()
                    lyric_updated = True

        if not lyric_updated:
            now = time.monotonic()
            if now - self.last_pinned_update_monotonic >= 5.0:
                await self._trigger_pinned_update(force_progress_update=True)

    def get_current_position_ms(self) -> int | None:
        pos_ms = None
        # 1. Primary: Use real-time parsed Ogg granules from our single HTTP reader thread
        if self.current_stream_granule_sec is not None and self.start_granule_sec is not None:
            # We subtract a buffer delay (approx. 1.0 second) to account for ffmpeg decoding,
            # resampling, Opus encoding, pipe buffering, and the Discord voice client queue.
            ffmpeg_buffer_delay = 1.0
            elapsed = time.monotonic() - self.current_stream_granule_time
            pos_sec = self.current_stream_granule_sec + elapsed - self.start_granule_sec - ffmpeg_buffer_delay
            pos_ms = int(max(0.0, pos_sec) * 1000)

        # 2. Secondary fallback: wall-clock elapsed time relative to stream_origin_sec
        elif self.start_granule_sec is not None and self.stream_origin_sec is not None:
            voice_client = None
            if self.voice_clients:
                voice_client = self.voice_clients[0]
            if voice_client and voice_client.is_playing() and self.voice_play_start_time is not None:
                elapsed = time.monotonic() - self.voice_play_start_time
            else:
                elapsed = 0.0
            pos_sec = self.stream_origin_sec + elapsed - self.start_granule_sec
            pos_ms = int(max(0.0, pos_sec) * 1000)

        # 3. Tertiary fallback: interpolated position from WebSocket
        elif self.last_position_received_at > 0.0:
            elapsed_sec = time.monotonic() - self.last_position_received_at
            pos_ms = int(max(0.0, (self.last_position_ms / 1000.0) + elapsed_sec) * 1000)

        if pos_ms is not None:
            return max(0, pos_ms + self.lyrics_offset_ms)

        return None

    def find_active_lyric_index(self, current_pos_ms: int) -> int | None:
        if not self.lyrics:
            return None
        active_idx = None
        for idx, line in enumerate(self.lyrics):
            if line.get("time_ms", 0) <= current_pos_ms:
                active_idx = idx
            else:
                break
        return active_idx

    def _get_lyrics_block(self) -> str | None:
        if not self.lyrics or not self.lyrics_track_id:
            return None
        if not self.last_track or self.last_track.track_id != self.lyrics_track_id:
            return None

        index = self.current_lyric_index if self.current_lyric_index is not None else -1

        displayed_lines = []
        for i in range(index - 3, index + 4):
            if 0 <= i < len(self.lyrics):
                line_text = _escape(self.lyrics[i].get("text", "")).strip()
                if not line_text:
                    line_text = "\u200b"
                if i == index:
                    displayed_lines.append(f"**{line_text}**")
                else:
                    displayed_lines.append(f"-# {line_text}")
            else:
                displayed_lines.append("-# \u200b")
        return "\n".join(displayed_lines)

    async def _trigger_pinned_update(self, *, force_progress_update: bool = False) -> None:
        if self.last_track is not None:
            await self._update_announcement_lyrics(
                self.last_track,
                accent=self.last_accent,
                force=False,
                force_progress_update=force_progress_update,
            )


def main() -> None:
    config_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_CONFIG_PATH

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    try:
        config = Config.load(config_path)
    except ConfigurationError as exc:
        raise SystemExit(f"Configuration error: {exc}") from exc

    if discord.version_info < (2, 7, 0):
        raise SystemExit(
            "discord.py 2.7 or newer is required. Run: "
            "python -m pip install -U 'discord.py[voice]>=2.7,<3'"
        )

    bot = SpotifmBot(config)
    bot.run(config.token, log_handler=None)


if __name__ == "__main__":
    main()
