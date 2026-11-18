#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
cargo build -p repo-city --bin vibe-city "$@"
target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
bundle="$project_dir/dist/Vibe City.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$target_dir/debug/vibe-city" "$bundle/Contents/MacOS/repo-city"
cp "$project_dir/assets/RepoCity.icns" "$bundle/Contents/Resources/RepoCity.icns"
licenses="$bundle/Contents/Resources/licenses"
mkdir -p "$licenses"
cp "$project_dir/LICENSE-MIT" "$project_dir/LICENSE-APACHE" "$project_dir/THIRD_PARTY.md" "$licenses/"
cp "$project_dir/licenses/"*.txt "$licenses/"
cp "$project_dir/assets/fonts/FiraMono-LICENSE" "$project_dir/assets/fonts/NotoEmoji-LICENSE" "$licenses/"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Vibe City</string>
  <key>CFBundleDisplayName</key><string>Vibe City</string>
  <key>CFBundleIdentifier</key><string>dev.meta-uber-engineer.repo-city</string>
  <key>CFBundleExecutable</key><string>repo-city</string>
  <key>CFBundleIconFile</key><string>RepoCity.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$bundle"
