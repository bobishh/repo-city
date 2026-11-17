#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
output="${1:-/tmp/repo-city-review-smoke}"
"$project_dir/scripts/build-macos.sh" --offline
python3 - "$project_dir" "$output" <<'PY'
from pathlib import Path
import plistlib, shutil, sys
root, output = map(Path, sys.argv[1:])
bundle = root / 'dist/RepoCitySmoke.app'
shutil.copytree(root / 'dist/Vibe City.app', bundle, dirs_exist_ok=True)
info_path = bundle / 'Contents/Info.plist'
with info_path.open('rb') as f:
    info = plistlib.load(f)
info.update(CFBundleName='RepoCitySmoke', CFBundleDisplayName='RepoCitySmoke', CFBundleIdentifier='dev.meta-uber-engineer.repo-city-smoke')
with info_path.open('wb') as f:
    plistlib.dump(info, f)
resources = bundle / 'Contents/Resources'
resources.mkdir(exist_ok=True)
(resources / 'smoke-output.txt').write_text(str(output.absolute()))
print(bundle)
PY
