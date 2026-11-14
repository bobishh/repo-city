#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --locked -p repo-city --profile web-release --target wasm32-unknown-unknown
output=web/dist
mkdir -p "$output/game"
cp web/index.html web/style.css web/app.mjs web/github.mjs "$output/"
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
