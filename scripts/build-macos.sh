#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
if [[ -z "${CARGO_TARGET_DIR:-}" && ! -d "$project_dir/target" && -d "$project_dir/../paket/target" ]]; then
  export CARGO_TARGET_DIR="$project_dir/../paket/target"
fi
cargo build -p repo-city --bin repo-city "$@"
target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
bundle="$project_dir/dist/Repo City.app"
mkdir -p "$bundle/Contents/MacOS"
cp "$target_dir/debug/repo-city" "$bundle/Contents/MacOS/repo-city"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Repo City</string>
  <key>CFBundleDisplayName</key><string>Repo City</string>
  <key>CFBundleIdentifier</key><string>dev.meta-uber-engineer.repo-city</string>
  <key>CFBundleExecutable</key><string>repo-city</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$bundle"
