#!/usr/bin/env python3
import os
import re
import sys
import socket
import ssl
import threading
import time
import json
import unicodedata
import urllib.parse
from typing import Any, Dict, List, Optional, Sequence, Tuple

import requests
import websocket

# ==============================================================================
# Configuration Settings (Placeholders for customization)
# ==============================================================================
IRC_HOST = "irc.example.org"        # Replace with your IRC host
IRC_PORT = 6667                     # Replace with your IRC port
IRC_USE_TLS = False                 # Enable for TLS (usually port 6697)
IRC_NICK = "RadioBot"               # Replace with your bot nickname
IRC_CHAN = "#channel"               # Replace with your IRC channel

SPOTIFM_API_URL = "http://localhost:3333"  # Replace with your Spotifm API URL
SPOTIFM_WS_URL = "ws://localhost:3333/ws"  # Replace with your Spotifm WS URL

# ==============================================================================
# Bot Control State Variables (Thread-Safe Toggles)
# ==============================================================================
lyrics_enabled = False
lyrics_latency_ms = 2000
lyrics_np_enabled = True

irc_sock = None
irc_lock = threading.Lock()

# ==============================================================================
# IRC Helper Functions
# ==============================================================================
def send_irc_raw(raw_message):
    """Send raw data to the IRC socket thread-safely."""
    global irc_sock
    with irc_lock:
        if irc_sock:
            try:
                irc_sock.send((raw_message + "\r\n").encode("utf-8"))
            except Exception as e:
                print(f"[IRC] Error sending raw data: {e}", file=sys.stderr)

def msg_chan(text):
    """Send text to the configured channel (splits multi-line responses)."""
    for line in text.split("\n"):
        line = line.strip()
        if line:
            send_irc_raw(f"PRIVMSG {IRC_CHAN} :{line}")

def connect_irc():
    """Open a blocking IRC connection, optionally protected by verified TLS."""
    sock = socket.create_connection((IRC_HOST, IRC_PORT), timeout=10)
    if not IRC_USE_TLS:
        sock.settimeout(None)
        return sock

    try:
        context = ssl.create_default_context()
        tls_sock = context.wrap_socket(sock, server_hostname=IRC_HOST)
    except Exception:
        sock.close()
        raise

    tls_sock.settimeout(None)
    return tls_sock

# ==============================================================================
# Spotifm API Helpers (Adapted from radio3.py / radio.py)
# ==============================================================================
VALID_CATEGORIES = ("track", "album", "artist", "playlist")
SORT_KEYS = ("artist", "album", "playlist")

def u(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        value = value.decode("utf-8", "replace")
    else:
        value = str(value)
    return (
        unicodedata.normalize("NFC", value)
        .encode("utf-8", "replace")
        .decode("utf-8", "replace")
        .replace("\r", " ")
        .replace("\n", " ")
        .replace("\t", " ")
    )

def enc(value: Any) -> str:
    return urllib.parse.quote(u(value), safe="")

def clean_spotify_id(category: str, object_id: str) -> str:
    object_id = u(object_id).strip()
    prefix = f"spotify:{category}:"
    if object_id.startswith(prefix):
        return object_id[len(prefix) :]
    if object_id.startswith("spotify:"):
        parts = object_id.split(":")
        if len(parts) == 3:
            return parts[2]
    return object_id

def parse_category(query: str) -> Tuple[str, str]:
    query = u(query).strip()
    if not query:
        return "track", ""

    parts = query.split()
    category = parts[0].lower()
    if category in VALID_CATEGORIES:
        return category, " ".join(parts[1:]).strip()
    return "track", query

def parse_media_reference(text: str) -> Optional[Tuple[str, str]]:
    text = u(text).strip()
    patterns = (
        r"spotify:(track|album|artist|playlist):([A-Za-z0-9_-]+)",
        r"(?:open\.spotify\.com/)?(track|album|artist|playlist)/([A-Za-z0-9_-]+)",
    )
    for pattern in patterns:
        match = re.search(pattern, text)
        if match:
            return match.group(1), match.group(2)
    return None

def parse_sort_args(query: str) -> Tuple[Optional[str], Optional[str]]:
    tokens = u(query).strip().split()
    if not tokens:
        return None, None

    lowered = [token.lower() for token in tokens]
    if "by" in lowered:
        idx = lowered.index("by")
        if idx + 1 < len(tokens) and lowered[idx + 1] in SORT_KEYS:
            sort_by = lowered[idx + 1]
            name_tokens = tokens[:idx] + tokens[idx + 2 :]
            return sort_by, " ".join(name_tokens).strip() or None

    if lowered[0] in SORT_KEYS:
        return lowered[0], " ".join(tokens[1:]).strip() or None

    if lowered[-1] in SORT_KEYS:
        return lowered[-1], " ".join(tokens[:-1]).strip() or None

    return None, None

def parse_int(value: str) -> Optional[int]:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None

class SpotifmApiError(Exception):
    def __init__(self, message: str, status_code: Optional[int] = None):
        super().__init__(message)
        self.status_code = status_code

class SpotifmClient:
    def __init__(self, base_url: Optional[str] = None, session: Optional[requests.Session] = None):
        configured = base_url or os.environ.get("SPOTIFM_BASE_URL") or SPOTIFM_API_URL
        self.base_url = configured.rstrip("/")
        self.session = session or requests.Session()

    def request(self, method: str, path: str, timeout: int = 10, **kwargs: Any) -> Any:
        try:
            response = self.session.request(
                method, f"{self.base_url}{path}", timeout=timeout, **kwargs
            )
        except requests.RequestException as exc:
            raise SpotifmApiError(str(exc)) from exc

        if response.status_code >= 400:
            message = response.text
            try:
                payload = response.json()
                if isinstance(payload, dict):
                    message = payload.get("error") or message
            except ValueError:
                pass
            raise SpotifmApiError(message, response.status_code)

        if not response.content:
            return {}
        try:
            return response.json()
        except ValueError as exc:
            raise SpotifmApiError(f"Invalid JSON response: {exc}") from exc

    def get(self, path: str, timeout: int = 10, params: Optional[Dict[str, Any]] = None) -> Any:
        return self.request("GET", path, timeout=timeout, params=params)

    def delete(self, path: str, timeout: int = 10, params: Optional[Dict[str, Any]] = None) -> Any:
        return self.request("DELETE", path, timeout=timeout, params=params)

# ==============================================================================
# Spotifm Command Handler (Adapted from radio3.py & radio.py)
# ==============================================================================
class SpotifmCommand:
    pattern = (
        r"^(?:!(playlists|playlist|queued|queue|search|skip|next|play|np|new|switch|"
        r"shuffle|sort|remove|help)(?:\s+(.*))?$|!add\s+(\S+)\s+(.*)|"
        r"\+(track|album|artist|playlist)\s+(\S+)\s+(.+))$"
    )

    def __init__(self, args=None, base_url: Optional[str] = None):
        self.args = args or ()
        self.base_url = base_url
        self.client = SpotifmClient(base_url)
        self._irc = True  # Default to True since we are inside an IRC bot

    def get_now_playing(self) -> str:
        """Use `/np` to get the current track, progress, active playlist, and listener count."""
        try:
            return self._format_now_playing(self.client.get("/np", timeout=5), include_progress=True)
        except Exception as exc:
            return self._error(exc)

    def get_next_track(self) -> str:
        """Use `/next` to get the next track selected by the Spotifm playback queue."""
        try:
            return self._format_now_playing(self.client.get("/next", timeout=5), prefix="Next")
        except Exception as exc:
            return self._error(exc)

    def search_spotify(self, category: str, query: str, limit: int = 5) -> str:
        """Use `/search/{category}/{limit}?q=...` to find Spotify tracks, albums, artists, or playlists."""
        try:
            self._validate_category(category)
            items = self.client.get(
                f"/search/{category}/{limit}", params={"q": query}, timeout=15
            )
            if not items:
                return f'No results for "{u(query)}"'
            return "\n".join(self._format_search_item(category, item) for item in items)
        except Exception as exc:
            return self._error(exc)

    def play_search(self, category: str, query: str, playlist_name: Optional[str] = None) -> str:
        """Use `/play/{category}?q=...` to search Spotify and play the first resolved result."""
        try:
            self._validate_category(category)
            params: Dict[str, Any] = {"q": query}
            if playlist_name:
                self._validate_playlist_name(playlist_name)
                params["playlist"] = playlist_name

            data = self.client.get(
                f"/play/{category}",
                params=params,
                timeout=self._resolve_timeout(category),
            )
            return self._format_now_playing(data, prefix="Playing")
        except Exception as exc:
            return self._error(exc)

    def queue_search(self, category: str, query: str) -> str:
        """Use `/queue/{category}?q=...` to search Spotify and queue the first resolved result."""
        try:
            self._validate_category(category)
            data = self.client.get(
                f"/queue/{category}",
                params={"q": query},
                timeout=self._resolve_timeout(category),
            )
            return self._format_now_playing(data, prefix="Queued")
        except Exception as exc:
            return self._error(exc)

    def play_item(
        self, category: str, object_id: str, playlist_name: Optional[str] = None
    ) -> str:
        """Use `/play/{category}/{id}` to play a Spotify object by ID or URI."""
        try:
            self._validate_category(category)
            raw_id = clean_spotify_id(category, object_id)
            params: Dict[str, Any] = {}
            if playlist_name:
                self._validate_playlist_name(playlist_name)
                params["playlist"] = playlist_name
            data = self.client.get(
                f"/play/{category}/{enc(raw_id)}",
                params=params,
                timeout=self._resolve_timeout(category),
            )
            return self._format_now_playing(data, prefix="Playing")
        except Exception as exc:
            return self._error(exc)

    def queue_item(self, category: str, object_id: str) -> str:
        """Use `/queue/{category}/{id}` to queue a Spotify object by ID or URI."""
        try:
            self._validate_category(category)
            raw_id = clean_spotify_id(category, object_id)
            data = self.client.get(
                f"/queue/{category}/{enc(raw_id)}",
                timeout=self._resolve_timeout(category),
            )
            return self._format_now_playing(data, prefix="Queued")
        except Exception as exc:
            return self._error(exc)

    def skip(self, count: Optional[int] = None, category: Optional[str] = None) -> str:
        """Use `/skip`, `/skip/{n}`, or `/skip/{artist|album}` to skip without client-side loops."""
        try:
            if count is not None and category is not None:
                raise SpotifmApiError("Cannot skip by count and category at the same time.")
            if category:
                if category not in ("artist", "album"):
                    raise SpotifmApiError("Skip category must be artist or album.")
                path = f"/skip/{category}"
            elif count is not None:
                path = f"/skip/{count}"
            else:
                path = "/skip"
            return self._format_now_playing(
                self.client.get(path, timeout=10), prefix="Now playing"
            )
        except Exception as exc:
            return self._error(exc)

    def get_queue(self) -> str:
        """Use `/queue` to list transient queued tracks in queue order."""
        try:
            return self._format_track_list(
                self.client.get("/queue", timeout=5),
                title="Transient Queue",
                empty="Queue is empty.",
                numbered=False,
            )
        except Exception as exc:
            return self._error(exc)

    def get_active_playlist(self) -> str:
        """Use `/playlist` to list tracks in the currently active playlist."""
        try:
            return self._format_track_list(
                self.client.get("/playlist", timeout=5),
                title="Active Playlist",
                empty="Playlist is empty.",
            )
        except Exception as exc:
            return self._error(exc)

    def list_playlists(self) -> str:
        """Use `/playlists` to list saved playlists and summary metadata."""
        try:
            playlists = self.client.get("/playlists", timeout=5)
            if not playlists:
                return "No playlists found."
            lines = ["Saved playlists:"]
            for playlist in playlists:
                if isinstance(playlist, str):
                    lines.append(f"- {u(playlist)}")
                    continue
                name = u(playlist.get("name") or playlist.get("playlist_name") or "")
                count = playlist.get("num_tracks", playlist.get("tracks", "?"))
                artists = ", ".join(u(item) for item in playlist.get("artists", [])[:3])
                suffix = f" - {artists}" if artists else ""
                lines.append(f"- {name}: {count} tracks{suffix}")
            return "\n".join(lines)
        except Exception as exc:
            return self._error(exc)

    def get_playlist(self, name: str) -> str:
        """Use `/playlist/{name}` to list tracks in a saved playlist by name."""
        try:
            self._validate_playlist_name(name)
            return self._format_track_list(
                self.client.get(f"/playlist/{enc(name)}", timeout=5),
                title=f"Playlist {name}",
                empty=f"Playlist {name} is empty.",
            )
        except Exception as exc:
            return self._error(exc)

    def switch_playlist(self, name: str) -> str:
        """Use `/playlist/switch/{name}` to activate or create a named playlist."""
        try:
            self._validate_playlist_name(name)
            tracks = self.client.get(f"/playlist/switch/{enc(name)}", timeout=5)
            return f"Switched to playlist {self._color(name, '1', '9')} with {len(tracks)} tracks."
        except Exception as exc:
            return self._error(exc)

    def shuffle_playlist(self, name: Optional[str] = None) -> str:
        """Use `/playlist/shuffle` or `/playlist/shuffle/{name}` to shuffle active or named playlist order."""
        try:
            if name:
                self._validate_playlist_name(name)
                tracks = self.client.get(f"/playlist/shuffle/{enc(name)}", timeout=10)
            else:
                tracks = self.client.get("/playlist/shuffle", timeout=10)
            target = f"playlist {u(name)}" if name else "active playlist"
            return f"Successfully shuffled {target} (contains {len(tracks)} tracks)."
        except Exception as exc:
            return self._error(exc)

    def sort_playlist(self, sort_by: str, name: Optional[str] = None) -> str:
        """Use `/playlist/sort?by=...` or `/playlist/{name}/sort?by=...` to sort by artist, album, or source playlist."""
        try:
            sort_by = u(sort_by).strip().lower()
            if sort_by not in SORT_KEYS:
                raise SpotifmApiError("Sort key must be artist, album, or playlist.")
            if name:
                self._validate_playlist_name(name)
                tracks = self.client.get(
                    f"/playlist/{enc(name)}/sort", params={"by": sort_by}, timeout=10
                )
                target = f"playlist {u(name)}"
            else:
                tracks = self.client.get(
                    "/playlist/sort", params={"by": sort_by}, timeout=10
                )
                target = "active playlist"
            return f"Successfully sorted {target} by {sort_by} (contains {len(tracks)} tracks)."
        except Exception as exc:
            return self._error(exc)

    def add_to_playlist(self, playlist_name: str, category: str, object_id: str) -> str:
        """Use `/playlist/{name}/add` to add a Spotify track, album, artist, or playlist by ID or URI."""
        try:
            self._validate_playlist_name(playlist_name)
            self._validate_category(category)
            key = f"{category}s[]"
            params = {key: clean_spotify_id(category, object_id)}
            data = self.client.get(
                f"/playlist/{enc(playlist_name)}/add",
                params=params,
                timeout=self._resolve_timeout(category),
            )
            added = data.get("added_tracks") if isinstance(data, dict) else None
            total = data.get("total_tracks") if isinstance(data, dict) else None
            if added is not None and total is not None:
                return (
                    f"Successfully added Spotify {category} to playlist "
                    f"{self._color(playlist_name, '1', '9')}. Added {added} tracks; "
                    f"playlist now contains {total} tracks."
                )
            return f"Successfully added Spotify {category} to playlist {self._color(playlist_name, '1', '9')}."
        except Exception as exc:
            return self._error(exc)

    def search_and_add_to_playlist(self, playlist_name: str, category: str, query: str) -> str:
        """Use `/search/{category}/1?q=...` then `/playlist/{name}/add` to add the first search result."""
        try:
            self._validate_category(category)
            results = self.client.get(
                f"/search/{category}/1", params={"q": query}, timeout=15
            )
            if not results:
                return f'No results for "{u(query)}"'
            item = results[0]
            object_id = item.get(f"{category}_id")
            if not object_id:
                return f"Search result did not include a {category}_id."
            details = self._describe_search_item(category, item)
            result = self.add_to_playlist(playlist_name, category, object_id)
            return result.replace(f"Spotify {category}", details)
        except Exception as exc:
            return self._error(exc)

    def remove_track_by_query(self, query: str) -> str:
        """Use `/search/track/1?q=...` then `DELETE /playlist/track/{id}` to remove the first matching active-playlist track."""
        try:
            tracks = self.client.get("/search/track/1", params={"q": query}, timeout=15)
            if not tracks:
                return f'No results for "{u(query)}"'
            track = tracks[0]
            track_id = track.get("track_id")
            remaining = self.client.delete(f"/playlist/track/{enc(track_id)}", timeout=10)
            name = u(track.get("track_name", ""))
            artists = ", ".join(u(item) for item in track.get("artists", []))
            return (
                f"Removed {self._bold(name)} by {self._bold(artists)} from the active playlist. "
                f"{len(remaining)} tracks remaining."
            )
        except Exception as exc:
            return self._error(exc)

    def help(self) -> str:
        """Return the direct user command reference for this radio toolbox entrypoint."""
        b = "\x02" if getattr(self, "_irc", False) else ""
        r = "\x0f" if getattr(self, "_irc", False) else ""
        lines = [
            f"{b}!np{r}: Show the current track playing",
            f"{b}!next{r}: Show the next track",
            f"{b}!skip [n|artist|album]{r}: Skip current track, signed count, artist, or album",
            f"{b}!queued{r}: List tracks in the queue",
            f"{b}!playlist{r}: List tracks in the active playlist",
            f"{b}!playlists{r}: List saved playlists",
            f"{b}!play [track|album|artist|playlist] <query>{r}: Play the first search result",
            f"{b}!queue [track|album|artist|playlist] <query>{r}: Queue the first search result",
            f"{b}!search [track|album|artist|playlist] <query>{r}: Search Spotify",
            f"{b}!switch <name>{r}: Switch to or create a playlist",
            f"{b}!shuffle [name]{r}: Shuffle active or named playlist",
            f"{b}!sort <artist|album|playlist> [name]{r}: Sort active or named playlist",
            f"{b}!remove <query>{r}: Remove a track from the active playlist",
            f"{b}!add <playlist> <link>{r}: Append a Spotify link",
            f"{b}+track|album|artist|playlist <playlist> <query>{r}: Search and add first result",
        ]
        if getattr(self, "_irc", False):
            lines.extend([
                f"{b}!lyrics <on|off>{r}: Enable/disable streaming lyrics",
                f"{b}!lyrics delay <ms>{r}: Change streaming lyrics latency offset",
                f"{b}!lyrics np <on|off>{r}: Toggle auto Now Playing announcement on song change"
            ])
        return "\n".join(lines)

    def dispatch_command_args(self, args: Sequence[Optional[str]], irc: bool = True) -> Optional[str]:
        """Route regex-captured user command arguments to the same toolbox methods."""
        self._irc = irc
        cmd = query = playlist_name = media_type = media_id = rest = ""

        if args and args[0] is not None:
            cmd = args[0] or ""
            query = args[1] if len(args) > 1 and args[1] else ""
        elif len(args) > 2 and args[2] is not None:
            cmd = "add"
            playlist_name = args[2] or ""
            rest = args[3] if len(args) > 3 and args[3] else ""
            parsed = parse_media_reference(rest)
            if not parsed:
                return self._error(
                    "Could not find a valid Spotify link or URI for track, album, artist, or playlist."
                )
            media_type, media_id = parsed
        elif len(args) > 4 and args[4] is not None:
            cmd = "+" + (args[4] or "")
            playlist_name = args[5] if len(args) > 5 and args[5] else ""
            query = args[6] if len(args) > 6 and args[6] else ""

        if cmd == "add":
            return self.add_to_playlist(playlist_name, media_type, media_id)
        if cmd.startswith("+") and cmd[1:] in VALID_CATEGORIES:
            return self.search_and_add_to_playlist(playlist_name, cmd[1:], query)
        if cmd == "np":
            return self.get_now_playing()
        if cmd == "skip":
            return self._skip_query(query)
        if cmd == "next":
            return self.get_next_track()
        if cmd == "play":
            return self._play_query(query)
        if cmd == "queue":
            return self._queue_query(query)
        if cmd == "queued":
            return self.get_queue()
        if cmd == "playlist":
            return self.get_active_playlist()
        if cmd == "playlists":
            return self.list_playlists()
        if cmd in ("new", "switch"):
            name = query.strip()
            return self.switch_playlist(name) if name else "Usage: !switch <playlist_name>"
        if cmd == "shuffle":
            return self.shuffle_playlist(query.strip() or None)
        if cmd == "sort":
            return self._sort_query(query)
        if cmd == "remove":
            q = query.strip()
            return self.remove_track_by_query(q) if q else "Usage: !remove <query>"
        if cmd == "search":
            category, search_query = parse_category(query)
            return self.search_spotify(category, search_query) if search_query else "Please provide a search query."
        if cmd == "help":
            return self.help()
        return None

    def _play_query(self, query: str) -> str:
        category, search_query = parse_category(query)
        if not search_query:
            return "Please provide a search query."
        return self.play_search(category, search_query)

    def _queue_query(self, query: str) -> str:
        category, search_query = parse_category(query)
        if not search_query:
            return "Please provide a search query."
        return self.queue_search(category, search_query)

    def _sort_query(self, query: str) -> str:
        sort_by, name = parse_sort_args(query)
        if not sort_by:
            return "Usage: !sort <artist|album|playlist> [playlist_name]"
        return self.sort_playlist(sort_by, name=name)

    def _skip_query(self, query: str) -> str:
        target = u(query).strip().lower()
        if not target:
            return self.skip()
        if target in ("artist", "album"):
            return self.skip(category=target)
        count = parse_int(target)
        if count is None:
            return "Usage: !skip [n|artist|album]"
        return self.skip(count=count)

    def _format_now_playing(
        self,
        data: Dict[str, Any],
        prefix: Optional[str] = None,
        include_progress: bool = False,
    ) -> str:
        status = u(data.get("status", "unknown"))
        name = u(data.get("track_name", ""))
        artists = ", ".join(u(item) for item in data.get("artists", []))
        active_playlist = u(data.get("active_playlist", ""))
        listeners = data.get("listeners", 0) or 0

        if not name:
            return f"{prefix or status.title()}: idle"

        line = self._format_track(name, artists)
        if prefix:
            line = f"{prefix}: {line}"

        if include_progress:
            progress = self._format_progress(data)
            if progress:
                line += f" {progress}"
        if listeners:
            line += f" {self._color(listeners, '1', '9')}" if getattr(self, "_irc", False) else f" ({listeners} listeners)"
        if active_playlist:
            line += f" \x0394[{active_playlist}]\x03" if getattr(self, "_irc", False) else f" [{active_playlist}]"
        return line

    def _format_progress(self, data: Dict[str, Any]) -> str:
        duration_ms = data.get("track_duration_ms") or 0
        position_ms = data.get("position_ms") or 0
        if duration_ms <= 0:
            return ""

        total_sec = duration_ms // 1000
        pos_sec = position_ms // 1000
        if not getattr(self, "_irc", False):
            return f"{pos_sec // 60}:{pos_sec % 60:02d}/{total_sec // 60}:{total_sec % 60:02d}"

        frac = position_ms / duration_ms
        bar_length = 20
        bar = ["─"] * bar_length
        idx = max(0, min(bar_length - 1, int(frac * bar_length)))
        bar[idx] = "O"
        return self._color(
            f"{pos_sec // 60}:{pos_sec % 60:02d} {''.join(bar)} {total_sec // 60}:{total_sec % 60:02d}",
            "1",
            "15",
        )

    def _format_track_list(
        self,
        tracks: Sequence[Dict[str, Any]],
        title: str,
        empty: str,
        numbered: bool = True,
    ) -> str:
        if not tracks:
            return empty
        lines = [f"{title}:"]
        for idx, track in enumerate(tracks, 1):
            artists = ", ".join(u(item) for item in track.get("artists", []))
            label = self._format_track(track.get("track_name", ""), artists)
            if numbered:
                lines.append(f"{idx}. {label}")
            else:
                queue_idx = track.get("queue_idx")
                prefix = f"{queue_idx}. " if queue_idx is not None else "- "
                lines.append(f"{prefix}{label}")
        return "\n".join(lines)

    def _format_search_item(self, category: str, item: Dict[str, Any]) -> str:
        return self._describe_search_item(category, item)

    def _describe_search_item(self, category: str, item: Dict[str, Any]) -> str:
        if category == "track":
            artists = ", ".join(u(value) for value in item.get("artists", []))
            return self._format_track(item.get("track_name", ""), artists)
        if category == "album":
            artists = ", ".join(u(value) for value in item.get("artists", []))
            return f'album "{u(item.get("album_name") or item.get("album_name", ""))}" by {artists}'
        if category == "artist":
            return f'artist "{u(item.get("artist_name", ""))}"'
        if category == "playlist":
            return f'playlist "{u(item.get("playlist_name", ""))}"'
        return u(item)

    def _format_track(self, name: Any, artists: Any) -> str:
        if getattr(self, "_irc", False):
            return f"\x031,9 ♪ \x03 \x031,15 {u(name)} - {u(artists)} \x03"
        artists = u(artists)
        return f"{u(name)} - {artists}" if artists else u(name)

    def _color(self, text: Any, fg: str = "1", bg: Optional[str] = None) -> str:
        text = u(text)
        if not getattr(self, "_irc", False):
            return text
        return f"\x03{fg},{bg}{text}\x03" if bg is not None else f"\x03{fg}{text}\x03"

    def _bold(self, text: Any) -> str:
        text = u(text)
        return f"\x02{text}\x02" if getattr(self, "_irc", False) else f"**{text}**"

    def _error(self, error: Any) -> str:
        message = u(error)
        if getattr(self, "_irc", False):
            return f"\x034Error: {message}\x03"
        return f"Error: {message}"

    @staticmethod
    def _resolve_timeout(category: str) -> int:
        return 180 if category in ("artist", "playlist") else 30

    @staticmethod
    def _validate_category(category: str) -> None:
        if category not in VALID_CATEGORIES:
            raise SpotifmApiError("Category must be track, album, artist, or playlist.")

    @staticmethod
    def _validate_playlist_name(name: str) -> None:
        if not name or not name.isalnum() or not name.isascii():
            raise SpotifmApiError("Playlist name must be ASCII alphanumeric.")

# ==============================================================================
# IRC Message Parsing & Dispatching Loop
# ==============================================================================
def parse_irc_message(raw_line):
    """Parse incoming IRC protocol line into sender and text."""
    prefix = ""
    trailing = []
    if not raw_line:
        return None, "", []

    if raw_line.startswith(':'):
        prefix, raw_line = raw_line[1:].split(' ', 1)

    if ' :' in raw_line:
        server_params, path = raw_line.split(' :', 1)
        args = server_params.split(' ')
        trailing.append(path)
    else:
        args = raw_line.split(' ')

    command = args[0]
    return prefix, command, args[1:] + trailing

def handle_channel_command(nickname, message):
    """Process bot commands originating from the IRC channel."""
    global lyrics_enabled, lyrics_latency_ms, lyrics_np_enabled

    # 1. Custom Interactive Lyrics Control Commands
    if message.startswith("!lyrics"):
        parts = message.split(" ")
        if len(parts) == 1:
            status = "enabled" if lyrics_enabled else "disabled"
            msg_chan(f"\x031,15 [Lyrics] Lyrics streaming is currently {status}. Delay offset is {lyrics_latency_ms}ms. Auto-announcement is {'enabled' if lyrics_np_enabled else 'disabled'}. \x03")
            return

        sub = parts[1].lower()
        if sub == "on":
            lyrics_enabled = True
            msg_chan("\x031,9 [Lyrics] Lyrics streaming enabled! \x03")
        elif sub == "off":
            lyrics_enabled = False
            msg_chan("\x031,4 [Lyrics] Lyrics streaming disabled! \x03")
        elif sub == "np" and len(parts) > 2:
            toggle = parts[2].lower()
            if toggle == "on":
                lyrics_np_enabled = True
                msg_chan("\x031,9 [Lyrics] Auto-announcements on song change enabled! \x03")
            elif toggle == "off":
                lyrics_np_enabled = False
                msg_chan("\x031,4 [Lyrics] Auto-announcements on song change disabled! \x03")
        elif sub == "delay" and len(parts) > 2:
            try:
                ms = int(parts[2])
                if ms >= 0:
                    lyrics_latency_ms = ms
                    msg_chan(f"\x031,9 [Lyrics] Synchronization latency delay set to {lyrics_latency_ms}ms. \x03")
                else:
                    msg_chan("\x031,4 [Error] Delay must be positive! \x03")
            except ValueError:
                msg_chan("\x031,4 [Error] Delay must be a valid integer in milliseconds! \x03")
        return

    # 2. Spotify Controller Commands via SpotifmCommand regex patterns
    match = re.match(SpotifmCommand.pattern, message)
    if match:
        cmd_handler = SpotifmCommand(args=match.groups(), base_url=SPOTIFM_API_URL)
        output = cmd_handler.dispatch_command_args(match.groups(), irc=True)
        if output:
            msg_chan(output)

# ==============================================================================
# Background Synced Lyrics WebSocket Client Thread
# ==============================================================================
def start_websocket_thread():
    """Background loop connecting to Spotifm WebSocket lyrics updates."""
    def run_websocket():
        while True:
            try:
                print(f"[WebSocket] Connecting to {SPOTIFM_WS_URL}...", flush=True)
                ws = websocket.create_connection(SPOTIFM_WS_URL)
                print("[WebSocket] Connected successfully!", flush=True)

                lines = []
                last_printed_idx = -1

                while True:
                    result = ws.recv()
                    if not result:
                        break

                    try:
                        msg = json.loads(result)
                    except Exception:
                        continue

                    mtype = msg.get("type")
                    if mtype == "Lyrics":
                        # Load and sort the synced lyrics lines by time_ms
                        lines = msg.get("lines", [])
                        lines.sort(key=lambda x: x.get("time_ms", 0))
                        last_printed_idx = -1

                        # Trigger automatic Now Playing announcement if enabled
                        if lyrics_np_enabled:
                            time.sleep(0.5) # Let Spotify playhead resolve

                            # Use SpotifmCommand to format now playing without progress bar
                            cmd_handler = SpotifmCommand(base_url=SPOTIFM_API_URL)
                            try:
                                data = cmd_handler.client.get("/np", timeout=5)
                                announcement = cmd_handler._format_now_playing(data, prefix="Now playing", include_progress=False)
                            except Exception as exc:
                                announcement = cmd_handler._error(exc)
                            msg_chan(announcement)

                    elif mtype == "Position":
                        if not lyrics_enabled:
                            continue

                        # Compensate for stream buffer latency by subtracting our offset
                        pos_ms = max(0, msg.get("position_ms", 0) - lyrics_latency_ms)

                        # Find the latest lyric line that has a timestamp <= current playback position
                        target_idx = -1
                        for idx, line in enumerate(lines):
                            if line.get("time_ms", 0) <= pos_ms:
                                target_idx = idx
                            else:
                                break

                        if target_idx != -1:
                            # Catch up silently if we just connected (last_printed_idx == -1) OR
                            # self-correct if the playhead jumps backward (target_idx < last_printed_idx)
                            if last_printed_idx == -1 or target_idx < last_printed_idx:
                                text = lines[target_idx].get("text", "").strip()
                                if text:
                                    msg_chan(f"\x031,9 ♪ \x03 \x031,15 {text} \x03")
                                last_printed_idx = target_idx
                            elif target_idx > last_printed_idx:
                                # Print subsequent lines in sync as their timing thresholds are crossed
                                for idx in range(last_printed_idx + 1, target_idx + 1):
                                    text = lines[idx].get("text", "").strip()
                                    if text:
                                        msg_chan(f"\x031,9 ♪ \x03 \x031,15 {text} \x03")
                                last_printed_idx = target_idx

                    elif mtype == "NoLyrics":
                        lines = []
                        last_printed_idx = -1

                    elif mtype == "Idle":
                        lines = []
                        last_printed_idx = -1

            except Exception as e:
                print(f"[WebSocket] Disconnected or error: {e}. Reconnecting in 5 seconds...", flush=True)
                time.sleep(5)

    t = threading.Thread(target=run_websocket, daemon=True)
    t.start()

# ==============================================================================
# Main IRC Connection Daemon
# ==============================================================================
def main():
    global irc_sock

    # Spawn background synced lyrics client
    start_websocket_thread()

    # Main socket connection reconnection loop
    while True:
        try:
            transport = "TLS" if IRC_USE_TLS else "plain TCP"
            print(f"[IRC] Connecting to {IRC_HOST}:{IRC_PORT} over {transport}...", flush=True)
            sock = connect_irc()

            with irc_lock:
                irc_sock = sock

            print(f"[IRC] Connection established! Authorizing as {IRC_NICK}...", flush=True)
            send_irc_raw(f"NICK {IRC_NICK}")
            send_irc_raw(f"USER {IRC_NICK} 0 * :Spotifm Lyrics Bot")

            buffer = ""
            joined_channel = False

            while True:
                data = sock.recv(4096)
                if not data:
                    print("[IRC] Socket disconnected by server.", flush=True)
                    break

                buffer += data.decode("utf-8", errors="ignore")
                lines = buffer.split("\r\n")
                buffer = lines.pop() # Keep the last incomplete block

                for line in lines:
                    line = line.strip()
                    if not line:
                        continue

                    # Handle server PING instantly to prevent timeouts
                    if line.startswith("PING"):
                        send_irc_raw(line.replace("PING", "PONG", 1))
                        continue

                    # Parse IRC protocol
                    prefix, command, args = parse_irc_message(line)

                    # Join channel once registration succeeds (Numeric 001 or MOTD End 376)
                    if command in ("001", "376") and not joined_channel:
                        print(f"[IRC] Registration complete. Joining {IRC_CHAN}...", flush=True)
                        send_irc_raw(f"JOIN {IRC_CHAN}")
                        joined_channel = True

                    # Handle channel public messages (PRIVMSG)
                    if command == "PRIVMSG" and len(args) >= 2:
                        target = args[0]
                        message = args[1]

                        # Extract sender nickname
                        sender_nick = prefix.split("!")[0] if "!" in prefix else prefix

                        # Process commands intended for the configured channel
                        if target.lower() == IRC_CHAN.lower():
                            handle_channel_command(sender_nick, message)

        except Exception as e:
            print(f"[IRC] Socket error: {e}. Reconnecting in 5 seconds...", flush=True)
            with irc_lock:
                irc_sock = None
            time.sleep(5)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nExiting bot. Goodbye!")
        sys.exit(0)
