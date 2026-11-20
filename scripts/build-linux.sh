#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$project_dir"
target="${CARGO_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$target" in *-linux-*) ;; *) printf 'Select a Linux Rust target with CARGO_BUILD_TARGET.\n' >&2; exit 1 ;; esac
profile="${VIBE_CITY_BUILD_PROFILE:-release}"
if [[ "${VIBE_CITY_SKIP_BUILD:-0}" != 1 ]]; then
    cargo build --locked -p repo-city --bin vibe-city --target "$target" --profile "$profile"
fi
profile_dir="$profile"
[[ "$profile" != dev ]] || profile_dir=debug
target_dir="${CARGO_TARGET_DIR:-$project_dir/target}"
binary="$target_dir/$target/$profile_dir/vibe-city"
[[ -x "$binary" ]] || { printf 'Missing Linux executable: %s\n' "$binary" >&2; exit 1; }
output="${VIBE_CITY_OUTPUT_DIR:-$project_dir/dist}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
name="RepoCity-$target"
bundle="$output/$name"
mkdir -p "$bundle/licenses"
cp "$binary" "$bundle/repo-city-bin"
cp LICENSE-MIT LICENSE-APACHE THIRD_PARTY.md "$bundle/licenses/"
cp licenses/*.txt assets/fonts/*-LICENSE "$bundle/licenses/"
cp web/assets/apple-touch-icon.png "$bundle/repo-city.png"
cat > "$bundle/repo-city" <<'LAUNCHER'
#!/usr/bin/env sh
set -eu
bundle_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$bundle_dir/repo-city-bin" "$@"
LAUNCHER
chmod +x "$bundle/repo-city"
cat > "$bundle/README.txt" <<'USAGE'
Run ./repo-city, then choose Open local repository from the Repo City logo.
Enter the checkout path, or launch ./repo-city --repo /path/to/checkout --live.
Git must be on PATH. The app needs a Vulkan-capable graphics driver.
Debian/Ubuntu runtime packages: git, xdg-utils, libasound2, libudev1,
libfontconfig1, libwayland-client0, libwayland-cursor0, libxkbcommon0,
libx11-6, libx11-xcb1, libxi6, libxrandr2, libxcursor1, libvulkan1.
System voice speech is currently available on macOS only.
USAGE
tar -czf "$output/$name.tar.gz" -C "$output" "$name"
printf '%s\n' "$output/$name.tar.gz"
