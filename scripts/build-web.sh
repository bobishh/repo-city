#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --locked -p repo-city --bin repo-city --profile web-release --target wasm32-unknown-unknown
output=web/dist
mkdir -p "$output/game" "$output/assets"
mkdir -p "$output/licenses"
cp LICENSE-MIT LICENSE-APACHE THIRD_PARTY.md licenses/pulldown-cmark-MIT.txt licenses/bevy-markup-MIT.txt licenses/bevy-markup-APACHE.txt licenses/MPL-2.0.txt "$output/licenses/"
cp assets/fonts/FiraMono-LICENSE assets/fonts/NotoEmoji-LICENSE web/assets/OFL-Caveat.txt "$output/licenses/"
cp web/index.html web/style.css web/app.mjs web/github.mjs web/analytics.mjs web/syntax.mjs web/github-picker.mjs web/pointer.mjs web/touch.mjs web/progress.mjs web/review.mjs web/pr-review.mjs web/tablet.mjs web/tablet-view.mjs web/audio.mjs web/audio-worklet.mjs "$output/"
rm -f "$output/github-markdown.css" "$output/github-markdown-LICENSE.txt"
# Keep the worker and each language out of the initial game download.
rm -rf "$output/syntax-chunks"
web/node_modules/.bin/esbuild web/syntax-worker.mjs --bundle --format=esm --splitting \
  --minify --legal-comments=linked --target=chrome122 --out-extension:.js=.mjs \
  --chunk-names='syntax-chunks/[name]-[hash]' --outdir="$output"
mkdir -p "$output/syntax-licenses"
for package in @shikijs/{core,primitive,langs,engine-javascript} oniguruma-to-es oniguruma-parser regex regex-recursion regex-utilities; do
  cp "web/node_modules/$package/LICENSE" "$output/syntax-licenses/${package##*/}.txt"
done
cp web/node_modules/@shikijs/vscode-textmate/LICENSE.md "$output/syntax-licenses/vscode-textmate.txt"
python3 - "$output/analytics-config.mjs" <<'PYCODE'
from pathlib import Path
import json
import os
import sys

key = os.environ.get("TINSINK_BROWSER_KEY", "").strip()
Path(sys.argv[1]).write_text(f"export const browserKey = {json.dumps(key)};\n")
PYCODE
cp web/assets/start-screen.webp web/assets/caveat-latin-wght.woff2 web/assets/OFL-Caveat.txt "$output/assets/"
cp web/assets/robot-head.svg web/assets/apple-touch-icon.png "$output/assets/"
wasm-bindgen --target web --remove-name-section --remove-producers-section \
  --out-dir "$output/game" --out-name repo_city \
  "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/web-release/repo-city.wasm"

python3 - "$output/game/repo_city_bg.wasm" <<'PYCODE'
from pathlib import Path
import gzip
import sys

path = Path(sys.argv[1])
packed = path.with_suffix(".wasm.gz")
if path.stat().st_size > 25 * 1024 * 1024:
    data = gzip.compress(path.read_bytes(), compresslevel=9, mtime=0)
    if len(data) > 25 * 1024 * 1024:
        raise SystemExit("Compressed WASM exceeds the Cloudflare static asset limit.")
    packed.write_bytes(data)
    path.unlink()
else:
    packed.unlink(missing_ok=True)
PYCODE
