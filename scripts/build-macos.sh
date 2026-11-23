#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
cargo build -p repo-city --bin vibe-city "$@"
target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
bundle="$project_dir/dist/Repo City.app"
mkdir -p "$project_dir/dist"
staging=$(mktemp -d "$project_dir/dist/.repo-city-build.XXXXXX")
trap 'rm -rf "$staging"' EXIT
candidate="$staging/Repo City.app"
mkdir -p "$candidate/Contents/MacOS" "$candidate/Contents/Resources"
cp "$target_dir/debug/vibe-city" "$candidate/Contents/MacOS/repo-city"
cp "$project_dir/assets/RepoCity.icns" "$candidate/Contents/Resources/RepoCity.icns"
licenses="$candidate/Contents/Resources/licenses"
mkdir -p "$licenses"
cp "$project_dir/LICENSE-MIT" "$project_dir/LICENSE-APACHE" "$project_dir/THIRD_PARTY.md" "$licenses/"
cp "$project_dir/licenses/"*.txt "$licenses/"
cp "$project_dir/assets/fonts/"*-LICENSE "$licenses/"
cat > "$candidate/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Repo City</string>
  <key>CFBundleDisplayName</key><string>Repo City</string>
  <key>CFBundleIdentifier</key><string>dev.meta-uber-engineer.repo-city</string>
  <key>CFBundleExecutable</key><string>repo-city</string>
  <key>CFBundleIconFile</key><string>RepoCity.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Use the microphone for a review room or coding agent after you enable voice.</string>
</dict></plist>
PLIST
codesign --force --sign - "$candidate"
codesign --verify --strict "$candidate"
# Swap directories so a running copy can keep its signed executable mapped.
python3 - "$candidate" "$bundle" <<'PY'
import ctypes
import errno
import os
import sys

candidate, bundle = map(os.fsencode, sys.argv[1:])
rename = ctypes.CDLL(None, use_errno=True).renamex_np
rename.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
rename_swap, rename_exclusive = 2, 4
while rename(candidate, bundle, rename_swap) != 0:
    error = ctypes.get_errno()
    if error != errno.ENOENT:
        raise OSError(error, os.strerror(error))
    if rename(candidate, bundle, rename_exclusive) == 0:
        break
    error = ctypes.get_errno()
    if error != errno.EEXIST:
        raise OSError(error, os.strerror(error))
PY
printf '%s\n' "$bundle"
