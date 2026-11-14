# Validation — 2026-11-13

## Confirmed

- Native macOS debug build passes (Bevy 0.20, Avian 0.8, Apple Silicon).
- `cargo test -p repo-city -p repo-world --offline`: 22 tests pass.
- `cargo fmt -p repo-world -p repo-city -- --check`: passes.
- Sloposcope snapshot `11ebbc7b33a3e8faf3cee8bc0ee155290bf3940e`
  produces 213 file objects in 42 directories.
- The desktop smoke run in `/tmp/repo-city-v3-smoke` completed with five
  screenshots and its success report. Pair, ghost and syntax-wall screenshots
  were visually inspected. They show syntax colors, aligned old/new source,
  insertion backgrounds, hunk blocks and the two version volumes.
- That smoke run used the SAME projectile-spawning path as player firing.
  Avian collision events detached the exact expected declaration, changed the
  private draft, spawned rigid fragments, and restoration recovered the original
  source and removed projectiles/fragments. These are runtime assertions,
  not a direct call to the deletion operation in the test harness.
- Unit tests independently cover falling, ground impact, bounce and settling;
  duplicate collision events; exact UTF-8 declaration removal; multiline syntax;
  additions/removals/replacements; and distant hunks in a 9,000-line file.
- A regression check verifies that a modified source outside `src/` takes
  precedence over an added file with no baseline.

## Visual findings and final verification limit

The successful smoke run exposed a camera inside the next row of buildings
in the detached-source shot. The camera now stays inside the reading street,
and the detached source panel is beside the original facade, instead of
projecting into the street. E now enters the source wall directly from an
anchored comparison; the next E returns to flight. The center reticle no
longer carries a filename over the source, and comparison targeting ignores
background city buildings while anchored.

After these final changes the application rebuilt successfully. Subsequent
native smoke screenshots were entirely black, including the UI. The harness
now rejects empty captures and retries; `/tmp/repo-city-v5-smoke/failure.txt`
records that no rendered frame arrived. Raising/focusing the smoke window did
not resolve this. The attempt to open the normal game window was interrupted.
The final build's visual behavior and manual keyboard interaction therefore
remain unverified. The cause of the black frames is not established; do not
report the final build as fully visually validated.

Re-run from a desktop session:

```sh
./scripts/run-native.sh ../sloposcope --smoke-dir /tmp/repo-city-smoke-new
# Or build the desktop smoke bundle, then open it normally:
./scripts/build-macos-smoke.sh /tmp/repo-city-smoke-new
```

Inspect the images as well as `passed.txt`: assertions verify state and physical
collision routing, while image inspection verifies readability and framing.
A `failure.txt` or empty capture is not a successful visual check. Use a fresh
output directory for each run.

## Scope

Rigid-body fragments and whole-declaration removal are implemented; structural
collapse, individual fragment reinsertion, arbitrary text editing and executable
puzzles are not. Source repositories are never modified. Drafts and review
markers are session-local; F6 explicitly exports a draft to this project's
sandbox. Ghost geometry is read-only and has no collider.
