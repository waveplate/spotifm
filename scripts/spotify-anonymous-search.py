#!/usr/bin/env python3
"""Try Spotify web-player searches without account or developer credentials.

Uses only the Python standard library and an in-memory visitor session.
Protocol reference:
https://github.com/sjdonado/idonthavespotify/blob/main/src/adapters/spotify.ts
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import http.cookiejar
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


PLAYER_URL = "https://open.spotify.com"
SEARCH_URL = "https://api-partner.spotify.com/pathfinder/v1/query"
SEARCH_HASH = "75bbf6bfcfdf85b8fc828417bfad92b7cd66bf7f556d85670f4da8292373ebec"
PLAYER_BUNDLE = re.compile(
    r'https://[^\s"\x27<>]+/(?:mobile-)?web-player\.[0-9a-f]+\.js'
)
TOTP_CONFIG = re.compile(
    r'\{\s*secret\s*:\s*["\x27]([^"\x27]+)["\x27]\s*,\s*version\s*:\s*(\d+)\s*\}'
)
SEARCH_TYPES = {
    "track": ("tracksV2", "tracks"),
    "album": ("albums",),
    "artist": ("artists",),
    "playlist": ("playlists",),
}


class ProbeError(Exception):
    pass


class SpotifyHttpError(ProbeError):
    def __init__(self, status: int, endpoint: str, retry_after: str | None):
        self.status = status
        self.retry_after = retry_after
        message = f"Spotify returned HTTP {status} from {endpoint}"
        if retry_after:
            message += f"; Retry-After: {retry_after} seconds"
        super().__init__(message)


class AnonymousSpotify:
    def __init__(self):
        # No browser cookies or credential files are imported or persisted.
        self.http = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar())
        )
        self.http.addheaders = [
            (
                "User-Agent",
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
                "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            )
        ]
        self._access_token: str | None = None
        self._expires_at = 0.0

    def _get(self, url: str, headers: dict | None = None) -> str:
        endpoint = urllib.parse.urlsplit(url)
        label = f"{endpoint.netloc}{endpoint.path}"
        try:
            request = urllib.request.Request(url, headers=headers or {})
            with self.http.open(request, timeout=25) as response:
                return response.read().decode("utf-8")
        except urllib.error.HTTPError as error:
            # Keep token values, cookies, and the TOTP query out of errors.
            raise SpotifyHttpError(
                error.code, label, error.headers.get("Retry-After")
            ) from None
        except (urllib.error.URLError, TimeoutError) as error:
            reason = getattr(error, "reason", error)
            raise ProbeError(f"Could not reach {label}: {reason}") from None

    def _get_json(self, url: str, headers: dict | None = None) -> dict:
        text = self._get(url, headers)
        try:
            result = json.loads(text)
        except json.JSONDecodeError:
            raise ProbeError("Spotify returned invalid JSON") from None
        if not isinstance(result, dict):
            raise ProbeError("Spotify returned an unexpected JSON response")
        return result

    def _visitor_token(self) -> str:
        if self._access_token and time.monotonic() < self._expires_at:
            return self._access_token

        html = self._get(PLAYER_URL + "/")
        bundle = PLAYER_BUNDLE.search(html)
        if not bundle:
            raise ProbeError("Could not find Spotify's current web-player bundle")
        javascript = self._get(bundle.group())
        configs = TOTP_CONFIG.findall(javascript)
        if not configs:
            raise ProbeError("Could not find visitor-token configuration in the player bundle")
        secret, version = max(configs, key=lambda item: int(item[1]))

        # Use Spotify's clock, including when the local system clock differs.
        server_time = self._get_json(PLAYER_URL + "/api/server-time").get("serverTime")
        if not isinstance(server_time, (int, float)):
            raise ProbeError("Spotify did not provide its server time")
        key = "".join(
            str(ord(char) ^ ((index % 33) + 9)) for index, char in enumerate(secret)
        ).encode("utf-8")
        counter = (int(server_time) // 30).to_bytes(8, "big")
        digest = hmac.new(key, counter, hashlib.sha1).digest()
        offset = digest[-1] & 15
        code = (int.from_bytes(digest[offset : offset + 4], "big") & 0x7FFFFFFF) % 1_000_000
        query = urllib.parse.urlencode(
            {
                "reason": "init",
                "productType": "web-player",
                "totp": f"{code:06d}",
                "totpVer": version,
                "ts": int(server_time),
            }
        )
        token = self._get_json(
            PLAYER_URL + "/api/token?" + query,
            {
                "Accept": "application/json",
                "Origin": PLAYER_URL,
                "Referer": PLAYER_URL + "/",
            },
        )
        if token.get("isAnonymous") is not True:
            raise ProbeError("Spotify did not issue an anonymous visitor token")
        access_token = token.get("accessToken")
        expiry = token.get("accessTokenExpirationTimestampMs")
        if not isinstance(access_token, str) or not access_token:
            raise ProbeError("Spotify did not issue a visitor access token")
        if not isinstance(expiry, (int, float)):
            raise ProbeError("Spotify did not provide the visitor token expiry")
        lifetime = expiry / 1000 - server_time
        if lifetime <= 30:
            raise ProbeError("Spotify returned a visitor token already near expiry")
        self._access_token = access_token
        self._expires_at = time.monotonic() + lifetime - 30
        return access_token

    def search(self, query: str, category: str = "track", limit: int = 5) -> list[dict]:
        variables = {
            "searchTerm": query,
            "offset": 0,
            "limit": limit,
            "numberOfTopResults": 5,
        }
        parameters = urllib.parse.urlencode(
            {
                "operationName": "searchDesktop",
                "variables": json.dumps(variables, separators=(",", ":")),
                "extensions": json.dumps(
                    {"persistedQuery": {"version": 1, "sha256Hash": SEARCH_HASH}},
                    separators=(",", ":"),
                ),
            }
        )
        for attempt in range(2):
            token = self._visitor_token()
            try:
                result = self._get_json(
                    SEARCH_URL + "?" + parameters,
                    {
                        "Authorization": "Bearer " + token,
                        "app-platform": "WebPlayer",
                        "Accept": "application/json",
                        "Origin": PLAYER_URL,
                        "Referer": PLAYER_URL + "/",
                    },
                )
            except SpotifyHttpError as error:
                if error.status == 401 and attempt == 0:
                    self._access_token = None
                    continue
                raise
            break

        data = result.get("data") or {}
        search = data.get("searchV2") or data.get("search")
        if not isinstance(search, dict):
            messages = [
                str(item.get("message", "Unknown error"))
                for item in (result.get("errors") or [])
                if isinstance(item, dict)
            ]
            message = "; ".join(messages)[:500] or "Missing search data"
            raise ProbeError("Spotify search failed: " + message.replace(token, "[redacted]"))
        for key in SEARCH_TYPES[category]:
            section = search.get(key)
            if isinstance(section, dict):
                items = section.get("items") or []
                break
        else:
            raise ProbeError(f"Spotify returned no {category} search section")
        rows = (public_result(item, category) for item in items)
        return [row for row in rows if row is not None][:limit]


def public_result(item: dict, category: str) -> dict | None:
    if not isinstance(item, dict):
        return None
    node = item
    for _ in range(3):
        child = next(
            (node[key] for key in ("track", "item", "data") if isinstance(node.get(key), dict)),
            None,
        )
        if child is None:
            break
        node = child
    uri = node.get("uri", "")
    name = node.get("name") or (node.get("profile") or {}).get("name")
    if not isinstance(uri, str) or not uri.startswith(f"spotify:{category}:") or not name:
        return None
    row = {"id": uri.rsplit(":", 1)[-1], "uri": uri, "name": name}
    artists = (node.get("artists") or {}).get("items", [])
    if artists:
        row["artists"] = [
            artist_name
            for artist in artists
            if (artist_name := (artist.get("profile") or {}).get("name") or artist.get("name"))
        ]
    album = node.get("album") or {}
    if album.get("uri"):
        row["album"] = {"uri": album["uri"], "name": album.get("name")}
    duration = (node.get("duration") or {}).get("totalMilliseconds")
    if isinstance(duration, int):
        row["duration_ms"] = duration
    images = node.get("images") or {}
    image_items = images.get("items") or []
    cover = (
        album.get("coverArt")
        or node.get("coverArt")
        or (node.get("visuals") or {}).get("avatarImage")
        or (image_items[0] if image_items else {})
        or {}
    )
    sources = [source for source in cover.get("sources", []) if source.get("url")]
    if sources:
        row["cover_url"] = max(sources, key=lambda image: image.get("width") or 0)["url"]
    return row


def search_limit(text: str) -> int:
    value = int(text)
    if not 1 <= value <= 50:
        raise argparse.ArgumentTypeError("limit must be between 1 and 50")
    return value


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("query", help='Search text, for example "Dr. Worm"')
    parser.add_argument("--type", choices=SEARCH_TYPES, default="track", dest="category")
    parser.add_argument("--limit", type=search_limit, default=5)
    args = parser.parse_args()
    if not args.query.strip():
        parser.error("query cannot be empty")
    client = AnonymousSpotify()
    try:
        results = client.search(args.query, args.category, args.limit)
    except (ProbeError, ValueError, OSError) as error:
        message = str(error)
        if client._access_token:
            message = message.replace(client._access_token, "[redacted]")
        print(json.dumps({"error": message}, ensure_ascii=False), file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "source": "spotify_web_player",
                "anonymous": True,
                "query": args.query,
                "type": args.category,
                "results": results,
            },
            ensure_ascii=False,
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
