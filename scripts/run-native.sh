#!/usr/bin/env bash
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
repository_path="${1:-$project_dir}"
if [[ $# -gt 0 ]]; then shift; fi
repository_path="$(cd "$repository_path" && pwd)"
cd "$project_dir"
if [[ -z "${CARGO_TARGET_DIR:-}" && ! -d "$project_dir/target" && -d "$project_dir/../paket/target" ]]; then
  export CARGO_TARGET_DIR="$project_dir/../paket/target"
fi
exec cargo run -p repo-city --bin repo-city -- --repo "$repository_path" "$@"
