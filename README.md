# Repo City

A local, native 3D code explorer built from the courier game's Bevy geometry.
The first world set is a dark city of vertical file blocks. Each block has a
filename, a color for its change status, and a surface for reading source.
Repository data and editing state are separate from presentation so another
set can use different objects and controls without creating another game.

## Run

```sh
./scripts/run-native.sh ../sloposcope
# Select another immutable snapshot:
./scripts/run-native.sh ../sloposcope --rev HEAD~1
# Start at a specific file:
./scripts/run-native.sh ../sloposcope --file src-tauri/src/parser.rs
```

The native build requires a Rust toolchain and a desktop graphics session. The
first build compiles Bevy.

On macOS, `./scripts/build-macos.sh` creates `dist/Repo City.app` for a normal
desktop launch. Click **Open Repository…** or press **⌘O**, then choose a
local Git checkout. Switch repositories from the same window. The app opens
the last successfully chosen folder on the next launch and reads its committed
HEAD. An invalid folder leaves the open city in place.

## Browser

The browser build opens public GitHub repositories from a URL or an
`owner/repository` name. Choose a revision and optionally a base ref. With a
base ref, the city compares the merge base against the chosen revision.
Without one, it compares the first parent. Files load as you approach them.
Use a browser with WebGPU enabled. The browser displays plain source and line
diffs. It uses GitHub's public API
and its unauthenticated rate limit.

```sh
rustup target add wasm32-unknown-unknown
# Install wasm-bindgen-cli at the wasm-bindgen version in Cargo.lock.
./scripts/build-web.sh
cd web
node tests/serve.mjs 18791
```

Open `http://127.0.0.1:18791`. Browser tests use a fixture for GitHub responses
and exercise the compiled WASM in Chromium.

The two-job CI and WASM asset packaging follow Paket at
`8a9cea4150f6adea385daa9ac411a1e5a8a2a1aa`. Repo City tests its macOS folder
picker build and uses direct GitHub requests, so its Worker only serves assets.
GitHub Actions runs native checks and uploads `browser-build` after the
browser checks pass. That artifact contains the Cloudflare Worker
configuration, handler and static build. The Worker serves the precompressed
module with its WASM content type when the raw file exceeds the static asset
limit. Successful runs on `main` deploy that same artifact to
`repo-city.meta-uber-engineer.dev` after both check jobs pass. The deployment
uses the `CLOUDFLARE_API_TOKEN` secret in the GitHub `production` environment.
The token needs Editor access to the `repo-city` Worker. CI publishes versions
to the existing custom domain. Manage domain bindings separately in Cloudflare.

## Explore

- **Codebase.** Start at a covered source listing. WASD flies, Space and left
  Ctrl change altitude, and Shift boosts. Hold the right mouse button to aim,
  or use Tab to capture the cursor and Esc to release it. Scroll or press
  Page Up / Page Down to glide along the facade at the current angle and
  distance. Open Files with P in the browser or ⌘P on macOS, type a filename
  or path, and press Enter to fly there.
  Click a file block from any side to fly to its source. Hold F to charge the
  water blaster and release to fire. A shot arcs downward and washes one strip
  of cover away on impact. Hitting another file block flies to it and reveals
  the hit strip when the source is ready. Revealed lines stay open on the
  same file and snapshot during the session. Home frames the file block.
  From a distance, each block shows an overview of its rows: blue-gray for
  covered, amber for partially revealed, and teal for revealed. Up close,
  the active wall shows a readable span of the selected file with ordinary
  syntax colors for exposed source. Scrolling moves through the listing;
  flying or looking around leaves that span in place.
  G turns to the right wall, which shows the diff against the base snapshot.
  The camera follows the corresponding line. Press G again to turn back.
- **Review.** Press C at a file to fly to its first change. W and S fly between
  code platforms. Large hunks span several platforms. Aim with the right mouse
  button and shoot a platform you already passed to clear it with a splash.
  You can still return to its code with S. W at the last platform lands
  at the complete file comparison. Scroll for aligned source, use [ and ] to
  jump between hunks, or press G to switch between pair and overlay. N flies to
  the next changed file. To review a local pull request, pass the base commit or
  merge base you want to compare against:

  ```sh
  ./scripts/run-native.sh ../sloposcope --base BASE --rev HEAD
  ```

- **Trace a bug.** Put repository-relative `path:line` frames in a text file,
  one per line, starting at the error. Launch with `--trace FILE` and press T.
  W and S follow the supplied stack in either direction. Each platform shows
  source around its recorded line in the selected revision.
- **Draft editing.** E opens a separate source wall. An aimed shot at a
  function block can detach a declaration into a private draft. R restores the
  original and clears debris. F6 exports a changed draft under this project's
  `sandbox/<commit>/<path>`. The inspected repository remains read-only.

## Data and presentation

`git ls-tree` builds the world without loading source bodies. Paths are grouped
by directory and assigned deterministic non-overlapping lots, with larger files
on each district's perimeter first. Snapshot sizes determine block heights
before any code loads. The map includes
the union of the selected commit and its comparison base, so deleted files
remain inspectable. The base defaults to the first parent. Source bodies and syntax trees load when a file is selected.
The inspected revision is resolved once to a commit ID; working-copy changes
cannot silently change a displayed source. Binary files and symlinks retain
their addresses but are not treated as source. Limits are explicit: 5,000 file
objects, 1 MiB per inspected UTF-8 source, eight function sections per page.

Function extraction is copied from Sloposcope's Tree-sitter anatomy profiles:
Rust, JS/TS, Ruby, Elixir and Elm. Unsupported languages still have source views;
they do not receive fabricated function geometry. Svelte markup is currently
source-only. Body size and branch measurements remain partial syntax facts.

Syntax highlighting parses the complete selected source with Tree-sitter before
splitting it into display pages, preserving multiline comments/strings and
UTF-8 offsets. Supported syntax: Rust, JS/JSX, TS/TSX, Ruby, Elixir and Elm.
Other languages remain plain text. Diff hunks come from local Git with external
diff drivers disabled. Replacements align old/new rows for reading; pairing
lines does not imply semantic equivalence or infer renames.

`world_sets/code-city.json` controls the active presentation. `--world-set PATH`
can load another facade palette. A new metaphor also needs a presentation
adapter; changing a palette alone does not implement birds, bugs or a library.
Those alternatives are not mixed into the city.

## Reused source

- Courier: rounded meshes, material helpers and
  native Bevy foundation. The courier sources were copied from a fixed committed
  revision so simultaneous courier edits are not affected.
- Sloposcope: anatomy records and `parser_metrics.rs`, including its tests.
  Byte ranges were added for exact declaration removal from private drafts.
- Caveat: Latin variable font from the same tincanban revision as the
  Gonzo Yuppie homepage, `0c7a08b7485a91bce3597c179408f58e5a57099c`.
  Repo City uses it for the browser cover title under the bundled
  [SIL Open Font License](web/assets/OFL-Caveat.txt).
- `docs/source-provenance.json` records source revisions and original hashes.

The original courier game remains under `crates/game` for subsequent reuse;
the new native entry point lives in its `src/repo_city` directory and is built
by the smaller `crates/explorer` package. OSM, delivery rules and networking
are not part of this explorer's runtime.

## Checks

```sh
cargo test -p repo-world
cargo test -p repo-city
cargo fmt -p repo-world -p repo-city -- --check
./scripts/run-native.sh ../sloposcope --smoke-dir /tmp/repo-city-smoke
```

This first version explores source, snapshots, functions and private deletions.
It does not prove program behavior, execute the inspected repository, fetch
hosted PRs, or automatically generate verified puzzles. Review markers are
session-local. Repository topology is stable within the selected comparison;
adding files or directories in another comparison can move the generated lots.

Physical impacts are rigid-body interactions; the chosen code declaration is
detached as a whole. This does not simulate structural engineering or prove a
behavioral defect. Individual restitution of sections and executable scenarios
remain future work. Drafts are discarded when switching file/snapshot unless
explicitly exported with F6; R restores the whole draft.

## License

Project source is available under either the [MIT license](LICENSE-MIT) or
the [Apache License 2.0](LICENSE-APACHE), at your option. See
[THIRD_PARTY.md](THIRD_PARTY.md) for fonts and map data under separate licenses.
