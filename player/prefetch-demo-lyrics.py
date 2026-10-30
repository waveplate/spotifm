#!/usr/bin/env python3
"""Fetch timed lyrics once and package them for the public static demo."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
import unicodedata
from urllib import error, parse, request


ROOT = Path(__file__).resolve().parent
API = "https://lrclib.net/api/"


def tracks_hash(tracks):
    def canonical(value):
        if isinstance(value, dict):
            return {key: canonical(item) for key, item in value.items()}
        if isinstance(value, list):
            return [canonical(item) for item in value]
        # Match JSON.stringify's representation of whole-number floats.
        if isinstance(value, float) and value.is_integer():
            return int(value)
        return value
    data = json.dumps(canonical(tracks), ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)
    return hashlib.sha256(data.encode("utf-8")).hexdigest()


def normalized(value):
    text = unicodedata.normalize("NFKD", str(value or "")).lower()
    text = "".join(char for char in text if not unicodedata.combining(char))
    return " ".join("".join(char if char.isalnum() else " " for char in text).split())


def artist_key(value):
    words = normalized(value.replace("$", "s")).split()
    return " ".join(sorted(word for word in words if word not in {"and", "feat", "featuring", "ft", "with", "x"}))


def timed_lyrics(result):
    lrc = result.get("syncedLyrics") if isinstance(result, dict) else None
    if not isinstance(lrc, str):
        return False
    return any(re.search(r"\[\d+:[0-5]\d(?:\.\d{1,3})?\]", row) and
               re.sub(r"\[[^\]]*\]", "", row).strip() for row in lrc.splitlines())


def rank(candidates, track):
    titles = {normalized(value) for value in [track["title"], *track.get("lyricsTitles", [])]}
    artists = [track["artist"], *track.get("artists", [])]
    artist_keys = {artist_key(value) for value in artists}
    artist_keys.add(artist_key(" ".join(track.get("artists") or [track["artist"]])))
    matches = [candidate for candidate in candidates if timed_lyrics(candidate) and
               normalized(candidate.get("trackName")) in titles and
               artist_key(candidate.get("artistName", "")) in artist_keys]
    def score(candidate):
        duration = candidate.get("duration")
        delta = abs(duration - track["durationSec"]) if isinstance(duration, (int, float)) and track.get("durationSec") else 0
        return (delta, normalized(candidate.get("albumName")) != normalized(track.get("album")))
    return sorted(matches, key=score)


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as file:
            temporary = Path(file.name)
            file.write(data)
        os.replace(temporary, path)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()


def fetch_json(path):
    url = API + path
    for attempt in range(5):
        try:
            req = request.Request(url, headers={"User-Agent": "spotifm-demo-builder/1.0", "Accept": "application/json"})
            with request.urlopen(req, timeout=20) as response:
                return json.load(response)
        except error.HTTPError as exc:
            if exc.code == 404:
                return None
            if attempt == 4 or exc.code not in (429, 500, 502, 503, 504):
                raise
            try:
                delay = min(30, max(1, int(exc.headers.get("Retry-After", 2 ** (attempt + 1)))))
            except ValueError:
                delay = 2 ** (attempt + 1)
            print(f"[Lyrics] HTTP {exc.code}; retrying in {delay}s", file=sys.stderr, flush=True)
            time.sleep(delay)
        except error.URLError:
            if attempt == 4:
                raise
            delay = 2 ** attempt
            print(f"[Lyrics] Network lookup failed; retrying in {delay}s", file=sys.stderr, flush=True)
            time.sleep(delay)
    return None


def lyric_result(track, cache_dir, refresh=False, offline=False):
    key = f'id-{track["lrclibId"]}' if track.get("lrclibId") else f'metadata-{tracks_hash([track])}'
    cache_path = cache_dir / f"{key}.json"
    if not refresh:
        try:
            cached = json.loads(cache_path.read_text(encoding="utf-8"))
            if timed_lyrics(cached) and (not track.get("lrclibId") or cached.get("id") == track["lrclibId"]):
                return cached
        except (OSError, ValueError):
            pass
    if offline:
        raise RuntimeError("Timed lyrics are not cached; run prefetch without --offline")
    if track.get("lrclibId"):
        result = fetch_json(f'get/{track["lrclibId"]}')
    else:
        params = {"track_name": track["title"], "artist_name": track["artist"]}
        result = None
        if track.get("album") and track.get("durationSec"):
            exact = fetch_json("get?" + parse.urlencode({**params, "album_name": track["album"], "duration": track["durationSec"]}))
            matches = rank([exact] if exact else [], track)
            result = matches[0] if matches else None
        if not result:
            candidates = {}
            artists = dict.fromkeys([track["artist"], track["artist"].replace("$", "s")])
            queries = [{**params, "artist_name": artist} for artist in artists]
            queries += [{"track_name": title} for title in dict.fromkeys([track["title"], *track.get("lyricsTitles", [])])]
            for query in queries:
                found = fetch_json("search?" + parse.urlencode(query)) or []
                for candidate in found:
                    if timed_lyrics(candidate):
                        candidates[candidate["id"]] = candidate
            matches = rank(list(candidates.values()), track)
            result = matches[0] if matches else None
    if not timed_lyrics(result):
        raise RuntimeError("No timed lyrics found; select a timed LRCLIB record in the song builder")
    if not isinstance(result.get("id"), int) or (track.get("lrclibId") and result["id"] != track["lrclibId"]):
        raise RuntimeError("LRCLIB returned an unexpected record ID")
    atomic_write(cache_path, json.dumps(result, ensure_ascii=False).encode("utf-8"))
    return result


def bundle(tracks, output, cache_dir, refresh=False, offline=False):
    # Commit the bundle only after every song has usable timed lyrics. A
    # provider failure must never replace a working bundle with partial data.
    records = [{"type": "manifest", "version": 1, "tracks_sha256": tracks_hash(tracks), "track_count": len(tracks)}]
    for index, track in enumerate(tracks):
        print(f'[Lyrics] {index + 1}/{len(tracks)}: {track["title"]}', flush=True)
        result = lyric_result(track, cache_dir, refresh, offline)
        records.append({"track_index": index, "source": "lrclib", "lrclib_id": result["id"],
                        "lyrics_base64": base64.b64encode(result["syncedLyrics"].encode("utf-8")).decode("ascii")})
    data = "".join(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n" for record in records).encode("utf-8")
    atomic_write(output, data)
    print(f'[Lyrics] Wrote {output} ({len(tracks)} tracks, {len(data)} bytes)', flush=True)


def load_bundle(path, tracks):
    records = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not records or records[0] != {"type": "manifest", "version": 1, "tracks_sha256": tracks_hash(tracks), "track_count": len(tracks)}:
        raise ValueError("Lyric bundle does not match this song selection; prefetch it again")
    by_index = {}
    for record in records[1:]:
        if not isinstance(record, dict):
            raise ValueError("Lyric bundle records must be objects")
        index = record.get("track_index")
        if type(index) is not int or not 0 <= index < len(tracks) or index in by_index:
            raise ValueError("Invalid or duplicate lyric bundle track index")
        lrc = base64.b64decode(record["lyrics_base64"], validate=True).decode("utf-8")
        if record.get("source") != "lrclib" or not timed_lyrics({"syncedLyrics": lrc}):
            raise ValueError("Bundle contains unusable timed lyrics")
        if type(record.get("lrclib_id")) is not int or record["lrclib_id"] <= 0:
            raise ValueError("Bundle contains an invalid LRCLIB record ID")
        if tracks[index].get("lrclibId") and tracks[index]["lrclibId"] != record["lrclib_id"]:
            raise ValueError("Bundle contains the wrong pinned LRCLIB record")
        by_index[index] = record
    if len(by_index) != len(tracks):
        raise ValueError("Lyric bundle is missing songs; prefetch it again")
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tracks", type=Path, default=ROOT / "demo-tracks.json")
    parser.add_argument("--output", type=Path, default=ROOT / "demo-lyrics.jsonl")
    parser.add_argument("--cache-dir", type=Path, default=ROOT / ".demo-lyrics-cache")
    parser.add_argument("--refresh", action="store_true", help="Refetch previously cached lyrics")
    parser.add_argument("--offline", action="store_true", help="Use only the local prefetch cache")
    args = parser.parse_args()
    try:
        tracks = json.loads(args.tracks.expanduser().read_text(encoding="utf-8"))
        if not isinstance(tracks, list) or not tracks:
            raise ValueError("The track file must contain a nonempty song list")
        bundle(tracks, args.output.expanduser(), args.cache_dir.expanduser(), args.refresh, args.offline)
    except (OSError, ValueError, KeyError, RuntimeError) as exc:
        print(f"[Lyrics] Prefetch failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
