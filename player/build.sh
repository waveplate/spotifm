#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$repo_root" "$@" <<'PY'
import base64
import argparse
import hashlib
import importlib.util
import json
import math
import pathlib
import re
import sys

repo_root = pathlib.Path(sys.argv[1])
parser = argparse.ArgumentParser(prog="player/build.sh")
parser.add_argument("--offline", action="store_true", help="Reuse embedded libraries without downloading assets")
build_mode = parser.add_mutually_exclusive_group()
build_mode.add_argument("--demo-only", action="store_true", help="Build the public demo and song builder only")
build_mode.add_argument("--builder-only", action="store_true", help="Build the authoring tool without requiring a lyric bundle")
parser.add_argument("--demo-tracks", type=pathlib.Path, default=repo_root / "player/demo-tracks.json",
                    help="JSON song selection exported by the demo builder")
parser.add_argument("--lyrics-bundle", type=pathlib.Path, default=repo_root / "player/demo-lyrics.jsonl",
                    help="Prefetched base64 lyric bundle matching the exported selection")
parser.add_argument("--prefetch-lyrics", action="store_true", help="Update the lyric bundle before building (cache only with --offline)")
parser.add_argument("--defer-lyrics", action="store_true", help="Prepare the demo while its lyric bundle is being downloaded separately")
args = parser.parse_args(sys.argv[2:])
offline = args.offline
demo_only = args.demo_only
builder_only = args.builder_only
tracks_path = args.demo_tracks.expanduser().resolve()
tracks = json.loads(tracks_path.read_text(encoding="utf-8"))
if not isinstance(tracks, list) or not tracks:
    raise SystemExit("Demo JSON must contain at least one sample")
for track in tracks:
    if not isinstance(track, dict):
        raise SystemExit("Every demo sample must be an object")
    for key in ("title", "artist"):
        if not isinstance(track.get(key), str) or not track[key].strip():
            raise SystemExit(f"Missing demo track field: {key}")
    if "spotifyId" in track and (not isinstance(track["spotifyId"], str) or not re.fullmatch(r"[A-Za-z0-9]{22}", track["spotifyId"])):
        raise SystemExit("spotifyId must be a 22-character Spotify track ID")
    preview_url = track.get("previewUrl")
    apple_song_id = track.get("appleSongId")
    apple_search = track.get("appleSearch", False)
    if type(apple_search) is not bool:
        raise SystemExit("appleSearch must be a boolean")
    if preview_url and (not isinstance(preview_url, str) or not preview_url.startswith("https://")):
        raise SystemExit("Demo audio must use an HTTPS URL")
    if apple_song_id is not None and (type(apple_song_id) is not int or apple_song_id <= 0):
        raise SystemExit("appleSongId must be a positive integer")
    if not preview_url and not apple_song_id and not apple_search:
        raise SystemExit("Each demo track needs previewUrl, appleSongId, or appleSearch")
    start = track.get("previewStartMs")
    if start is not None and (type(start) not in (int, float) or not math.isfinite(start)):
        raise SystemExit("previewStartMs must be a finite signed number or null")
    duration = track.get("durationSec")
    if duration is not None and (type(duration) not in (int, float) or not math.isfinite(duration) or duration <= 0):
        raise SystemExit("durationSec must be a positive number")
    if "lrclibId" in track and (type(track["lrclibId"]) is not int or track["lrclibId"] <= 0):
        raise SystemExit("lrclibId must be a positive integer")
    for field in ("artists", "lyricsTitles"):
        if field in track and (not isinstance(track[field], list) or
                               any(not isinstance(value, str) or not value.strip() for value in track[field])):
            raise SystemExit(f"{field} must contain nonempty names")

spec = importlib.util.spec_from_file_location("demo_lyrics", repo_root / "player/prefetch-demo-lyrics.py")
demo_lyrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(demo_lyrics)
bundle_path = args.lyrics_bundle.expanduser().resolve()
bundle_data = None
if not builder_only:
    try:
        if args.prefetch_lyrics:
            demo_lyrics.bundle(tracks, bundle_path, repo_root / "player/.demo-lyrics-cache", offline=offline)
        try:
            demo_lyrics.load_bundle(bundle_path, tracks)
        except (OSError, ValueError, KeyError, TypeError):
            if offline:
                raise
            demo_lyrics.bundle(tracks, bundle_path, repo_root / "player/.demo-lyrics-cache")
            demo_lyrics.load_bundle(bundle_path, tracks)
        bundle_data = bundle_path.read_bytes()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        if args.defer_lyrics:
            print(f"[Build] Lyric bundle pending: {error}", file=sys.stderr)
            print("[Build] Prepared demo will show lyrics only after demo-lyrics.jsonl is supplied.", file=sys.stderr)
        else:
            raise SystemExit(f"[Build] Cannot build demo lyrics: {error}\n"
                             f"Run python3 player/prefetch-demo-lyrics.py --tracks {tracks_path}\n"
                             "Then rebuild, or use --builder-only to build the authoring tool.")
index_output_path = repo_root / "player/index.html"
previous_index = index_output_path.read_text(encoding="utf-8") if index_output_path.exists() else ""

# Define input paths
defaults_path = repo_root / "player/defaults.json"
css_path = repo_root / "player/src/css/000-player.css"
js_path = repo_root / "player/src/js/000-player.js"
vendor_js_path = repo_root / "player/vendor/ogg-vorbis-decoder.min.js"
worklet_js_path = repo_root / "player/spotifm-audio-worklet.js"

# Read dependencies
css = css_path.read_text(encoding="utf-8")
js = js_path.read_text(encoding="utf-8")

# Parse and increment the version number in 000-player.js
version_match = re.search(r'const\s+SPOTIFM_PLAYER_VERSION\s*=\s*(\d+);', js)
if version_match and not demo_only and not builder_only:
    current_version = int(version_match.group(1))
    next_version = current_version + 1
    js = js.replace(version_match.group(0), f'const SPOTIFM_PLAYER_VERSION = {next_version};')
    js_path.write_text(js, encoding="utf-8")
    print(f"[Build] Incremented player version from {current_version} to {next_version}")
elif not version_match:
    print("[Warning] Could not find const SPOTIFM_PLAYER_VERSION declaration in 000-player.js")

defaults = json.loads(defaults_path.read_text(encoding="utf-8"))

# Read binary/sensitive code as bytes and encode to base64
vendor_js_b64 = base64.b64encode(vendor_js_path.read_bytes()).decode("utf-8")
worklet_js_b64 = base64.b64encode(worklet_js_path.read_bytes()).decode("utf-8")

if not isinstance(defaults, dict):
    raise SystemExit("player/defaults.json must contain a JSON object")

# Cache directory for remote vendor files
vendor_dir = repo_root / "player/vendor"
vendor_dir.mkdir(parents=True, exist_ok=True)

butterchurn_local_path = vendor_dir / "butterchurn.js"
presets_local_path = vendor_dir / "butterchurn-presets.js"

# Download Butterchurn if not present
if not offline and not butterchurn_local_path.exists():
    print("[Build] Downloading Butterchurn module...")
    try:
        import urllib.request
        url = "https://unpkg.com/butterchurn@3.0.0-beta.5/dist/butterchurn.js"
        with urllib.request.urlopen(url, timeout=20) as response:
            butterchurn_local_path.write_bytes(response.read())
    except Exception as e:
        print(f"[Build] Warning: Failed to download Butterchurn: {e}")

# Download Presets if not present
if not offline and not presets_local_path.exists():
    print("[Build] Downloading Butterchurn presets...")
    try:
        import urllib.request
        url = "https://unpkg.com/butterchurn-presets@3.0.0-beta.4/dist/base.js"
        with urllib.request.urlopen(url, timeout=30) as response:
            presets_local_path.write_bytes(response.read())
    except Exception as e:
        print(f"[Build] Warning: Failed to download Butterchurn presets: {e}")

# Base64 encode the files
butterchurn_b64 = ""
presets_b64 = ""
if butterchurn_local_path.exists():
    butterchurn_b64 = base64.b64encode(butterchurn_local_path.read_bytes()).decode("utf-8")
if presets_local_path.exists():
    presets_b64 = base64.b64encode(presets_local_path.read_bytes()).decode("utf-8")

# Reuse the libraries embedded in a previous build when downloads are unavailable.
def previous_library(name):
    match = re.search(r'const ' + name + r' = "([A-Za-z0-9+/=]+)";', previous_index)
    return match.group(1) if match else ""

butterchurn_b64 = butterchurn_b64 or previous_library("inlineButterchurnBase64")
presets_b64 = presets_b64 or previous_library("inlinePresetsBase64")

# Inline the base64 worklet and libraries into main player JS
js_with_worklet = js.replace("__SPOTIFM_AUDIO_WORKLET_BASE64__", worklet_js_b64)
js_with_worklet = js_with_worklet.replace("__BUTTERCHURN_CODE_BASE64__", butterchurn_b64)
js_with_worklet = js_with_worklet.replace("__BUTTERCHURN_PRESETS_CODE_BASE64__", presets_b64)

# Fetch Google Fonts list for autocomplete
print("[Build] Fetching Google Fonts metadata...")
google_fonts_url = "https://cdn.jsdelivr.net/npm/google-font-metadata@latest/data/google-fonts-v2.json"
google_fonts_options = ""
try:
    if offline:
        raise RuntimeError("offline build")
    import urllib.request
    with urllib.request.urlopen(google_fonts_url, timeout=10) as response:
        fonts_data = json.loads(response.read().decode('utf-8'))
    font_names = sorted([font["family"] for font in fonts_data.values()])
    google_fonts_options = "\n".join(f'        <option value="{name}"></option>' for name in font_names)
    print(f"[Build] Successfully retrieved {len(font_names)} font families.")
except Exception as e:
    print(f"[Build] Warning: Failed to fetch Google Fonts list: {e}")
    fallback_fonts = ["Roboto", "Open Sans", "Lato", "Montserrat", "Oswald", "Source Sans Pro", "Slabo 27px", "Raleway", "PT Sans", "Merriweather"]
    previous_fonts = re.search(r'<datalist id="google-fonts-list">(.*?)</datalist>', previous_index, flags=re.S)
    google_fonts_options = previous_fonts.group(1) if previous_fonts else "\n".join(f'        <option value="{name}"></option>' for name in fallback_fonts)

# 1. Build index.html
index_template_path = repo_root / "player/src/index.html"
index_template = index_template_path.read_text(encoding="utf-8")

css_tag = '<link rel="stylesheet" href="./css/000-player.css">'
js_tag = '<script src="./js/000-player.js"></script>'
vendor_tag = '<script src="{player_assets_endpoint}/vendor/ogg-vorbis-decoder.min.js" charset="UTF-8"></script>'

if css_tag not in index_template:
    raise SystemExit("Missing CSS tag in player/src/index.html")
if js_tag not in index_template:
    raise SystemExit("Missing JS tag in player/src/index.html")
if vendor_tag not in index_template:
    raise SystemExit("Missing vendor script tag in player/src/index.html")

rendered_index = index_template.replace(
    "__PLAYER_DEFAULTS_JSON__",
    json.dumps(defaults, separators=(",", ":")),
)
rendered_index = rendered_index.replace(
    "__GOOGLE_FONTS_DATALIST_OPTIONS__",
    google_fonts_options,
)
rendered_index = rendered_index.replace(
    css_tag,
    "<style>\n" + css.replace("</style>", "<\\/style>") + "\n    </style>",
)
rendered_index = rendered_index.replace(
    js_tag,
    "<script>\n" + js_with_worklet.replace("</script>", "<\\/script>") + "\n    </script>",
)

# Inline ogg-vorbis-decoder using Base64 script injection to prevent line-ending corruption of yenc
loader_js = f"""<script id="ogg-vorbis-decoder-loader">
(function(){{
  const base64 = "{vendor_js_b64}";
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {{
    bytes[i] = binary.charCodeAt(i);
  }}
  const el = document.createElement("script");
  el.textContent = new TextDecoder("utf-8").decode(bytes);
  document.currentScript.parentNode.insertBefore(el, document.currentScript);
}})();
</script>"""

rendered_index = rendered_index.replace(vendor_tag, loader_js)

if not demo_only and not builder_only:
    index_output_path.write_text(rendered_index, encoding="utf-8")
    print("[Build] Built player/index.html successfully!")

media_js = (repo_root / "player/src/js/demo-media.js").read_text(encoding="utf-8")
catalog_js = (repo_root / "player/src/js/demo-catalog.js").read_text(encoding="utf-8")
builder = (repo_root / "player/src/demo-builder.html").read_text(encoding="utf-8")
builder = builder.replace("__DEMO_TRACKS_JSON__", json.dumps(tracks, separators=(",", ":"), allow_nan=False).replace("<", "\\u003c"))
builder_css = (repo_root / "player/src/css/demo-builder.css").read_text(encoding="utf-8")
builder_js = (repo_root / "player/src/js/demo-builder.js").read_text(encoding="utf-8")
builder = builder.replace('<link rel="stylesheet" href="./css/demo-builder.css">',
                          '<style>\n' + builder_css.replace("</style>", "<\\/style>") + '\n</style>')
for name, source in (("demo-media", media_js), ("demo-catalog", catalog_js), ("demo-builder", builder_js)):
    builder = builder.replace(f'<script src="./js/{name}.js"></script>',
                              '<script>\n' + source.replace("</script>", "<\\/script>") + '\n</script>')
(repo_root / "player/demo-builder.html").write_text(builder, encoding="utf-8")
print("[Build] Built player/demo-builder.html successfully!")
if builder_only:
    sys.exit(0)

# The demo fetches this static bundle once; it contains no live lyric client.
bundle_output = repo_root / "player/demo-lyrics.jsonl"
if bundle_data is not None and bundle_path != bundle_output.resolve():
    demo_lyrics.atomic_write(bundle_output, bundle_data)
bundle_digest = hashlib.sha256(bundle_data).hexdigest() if bundle_data is not None else demo_lyrics.tracks_hash(tracks)
# Standalone demo: the same player UI and visual engine, with a browser source.
demo_defaults = {**defaults, "autosync": True, "autoSyncLyricsOffset": 0,
                 "delay": 0, "apiKeyEnabled": False, "apiKey": "",
                 "showListenerNumber": False}
demo_config = {"demo": {"tracks": tracks, "lyricsUrl": f"demo-lyrics.jsonl?v={bundle_digest[:16]}",
                        "lyricsTracksHash": demo_lyrics.tracks_hash(tracks)}, "streamIsOgg": False,
               "streamIsPassthrough": False, "projectDefaults": demo_defaults}
demo_js = (repo_root / "player/src/js/demo-source.js").read_text(encoding="utf-8")
demo_css = (repo_root / "player/src/css/demo.css").read_text(encoding="utf-8")
demo = rendered_index.replace("<title>Spotifm | Web Player</title>", "<title>Spotifm | Player Demo</title>")
demo = demo.replace("<body>", '<body class="spotifm-demo">')
demo = demo.replace("</head>", "<style>\n" + demo_css + "\n</style>\n</head>")
demo = demo.replace('data-listen-endpoint="{listen_endpoint}"', 'data-listen-endpoint=""')
demo = demo.replace(loader_js, "")  # Native MP3 playback needs no Ogg decoder.
config_pattern = r'<script id="player-runtime-config" type="application/json">.*?</script>'
demo, config_count = re.subn(config_pattern, lambda _: '<script id="player-runtime-config" type="application/json">' +
              json.dumps(demo_config, separators=(",", ":")).replace("<", "\\u003c") + '</script>\n<script>\n' +
              media_js.replace("</script>", "<\\/script>") + '\n</script>\n<script>\n' +
              demo_js.replace("</script>", "<\\/script>") + '\n</script>', demo, count=1, flags=re.S)
if config_count != 1:
    raise SystemExit("Missing player-runtime-config script in player/src/index.html")
(repo_root / "player/demo.html").write_text(re.sub(r"[ \t]+$", "", demo, flags=re.M), encoding="utf-8")
print("[Build] Built player/demo.html successfully!")

if demo_only:
    sys.exit(0)

# 2. Build minimal.html
minimal_template_path = repo_root / "player/src/minimal.html"
minimal_output_path = repo_root / "player/minimal.html"
minimal_template = minimal_template_path.read_text(encoding="utf-8")

if vendor_tag not in minimal_template:
    raise SystemExit("Missing vendor script tag in player/src/minimal.html")

rendered_minimal = minimal_template.replace(vendor_tag, loader_js)
rendered_minimal = rendered_minimal.replace(
    "__SPOTIFM_AUDIO_WORKLET_BASE64__",
    worklet_js_b64
)

minimal_output_path.write_text(rendered_minimal, encoding="utf-8")
print("[Build] Built player/minimal.html successfully!")
PY
