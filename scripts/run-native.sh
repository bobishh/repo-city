#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
repository_path="${1:-$project_dir}"
if [[ $# -gt 0 ]]; then shift; fi
repository_path="$(cd "$repository_path" && pwd)"
cd "$project_dir"
exec cargo run -p repo-city --bin vibe-city -- --repo "$repository_path" "$@"
