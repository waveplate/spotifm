#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 - "$repo_root" <<'PY'
import base64
import json
import pathlib
import re
import sys

repo_root = pathlib.Path(sys.argv[1])

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
if version_match:
    current_version = int(version_match.group(1))
    next_version = current_version + 1
    js = js.replace(version_match.group(0), f'const SPOTIFM_PLAYER_VERSION = {next_version};')
    js_path.write_text(js, encoding="utf-8")
    print(f"[Build] Incremented player version from {current_version} to {next_version}")
else:
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
if not butterchurn_local_path.exists():
    print("[Build] Downloading Butterchurn module...")
    try:
        import urllib.request
        url = "https://unpkg.com/butterchurn@3.0.0-beta.5/dist/butterchurn.js"
        with urllib.request.urlopen(url, timeout=20) as response:
            butterchurn_local_path.write_bytes(response.read())
    except Exception as e:
        print(f"[Build] Warning: Failed to download Butterchurn: {e}")

# Download Presets if not present
if not presets_local_path.exists():
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

# Inline the base64 worklet and libraries into main player JS
js_with_worklet = js.replace("__SPOTIFM_AUDIO_WORKLET_BASE64__", worklet_js_b64)
js_with_worklet = js_with_worklet.replace("__BUTTERCHURN_CODE_BASE64__", butterchurn_b64)
js_with_worklet = js_with_worklet.replace("__BUTTERCHURN_PRESETS_CODE_BASE64__", presets_b64)

# Fetch Google Fonts list for autocomplete
print("[Build] Fetching Google Fonts metadata...")
google_fonts_url = "https://cdn.jsdelivr.net/npm/google-font-metadata@latest/data/google-fonts-v2.json"
google_fonts_options = ""
try:
    import urllib.request
    with urllib.request.urlopen(google_fonts_url, timeout=10) as response:
        fonts_data = json.loads(response.read().decode('utf-8'))
    font_names = sorted([font["family"] for font in fonts_data.values()])
    google_fonts_options = "\n".join(f'        <option value="{name}"></option>' for name in font_names)
    print(f"[Build] Successfully retrieved {len(font_names)} font families.")
except Exception as e:
    print(f"[Build] Warning: Failed to fetch Google Fonts list: {e}")
    fallback_fonts = ["Roboto", "Open Sans", "Lato", "Montserrat", "Oswald", "Source Sans Pro", "Slabo 27px", "Raleway", "PT Sans", "Merriweather"]
    google_fonts_options = "\n".join(f'        <option value="{name}"></option>' for name in fallback_fonts)

# 1. Build index.html
index_template_path = repo_root / "player/src/index.html"
index_output_path = repo_root / "player/index.html"
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

index_output_path.write_text(rendered_index, encoding="utf-8")
print("[Build] Built player/index.html successfully!")

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
