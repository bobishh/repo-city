# Repo City development

## Local Git and source inspection

The native app can read immutable commits from a local Git checkout. Use
`--rev REF` to select a commit, branch, or tag. Pass `--base REF` to compare
with that revision directly. To review a branch from its merge base, resolve
the base with Git before launching.

```sh
./scripts/run-native.sh . --rev HEAD --base HEAD~1 --file web/app.mjs
```

The native Live worktree mode compares on-disk content with `HEAD`. It follows
staged and unstaged changes and untracked files that Git does not ignore. The
watcher groups filesystem events and assembles snapshots on a background
thread. The city keeps existing file positions and animates building height
changes when it switches snapshots. The watcher follows the batching pattern
from Sloposcope at `11ebbc7b33a3e8faf3cee8bc0ee155290bf3940e`. Repo City
passes snapshots to its scene instead of maintaining a graph in SQLite.

Use **Tab → Trace** to paste a stack trace on desktop or in the browser.
For command-line loading, save the trace to a file and pass `--trace FILE`.
Press **T** in the city; **W** and **S** follow the supplied frame order.
The same importer handles both entry points. It accepts repository-relative
`path:line` frames and common runtime stack formats; unresolved locations appear
in the tablet's Trace preview.

Press **Enter** to enter the source wall and left-click a function to detach its
declaration into a private draft. **R** restores it. **F6** explicitly exports
a changed draft under `sandbox/<commit>/<path>` in the Repo City project.
The app reads the inspected checkout without modifying it. Drafts live in
the current session. Revealed ranges and review marks are keyed by checkout
or GitHub repository, revision, and comparison base. The browser uses
IndexedDB and the Mac app stores `progress.sqlite3` in
`~/Library/Application Support/Repo City/`. Reset clears the active
comparison in the current browser profile or desktop installation.

The city uses directory paths and file metadata to position buildings.
Source loads on demand. The native app uses Tree-sitter to highlight Rust,
JavaScript, TypeScript, Ruby, Elixir, and Elm. The browser loads the complete
pinned Starry Night TextMate catalog in a Web Worker, including Lean 4. Grammars
and their dependencies load separately when needed. Unknown formats show plain
text. Native function extraction and its structural measurements
cover supported languages. The city streams up to 5,000 nearby buildings while search covers the full
comparison. UTF-8 source inspection is limited to 1 MiB per file. Binaries and symlinks keep their positions
in the city.

The active presentation is configured in
[`world_sets/code-city.json`](../world_sets/code-city.json). Source revisions
and hashes for reused code are recorded in
[`source-provenance.json`](source-provenance.json). The city uses the copied
`rounded_box` mesh for buildings and the Android tool model. The mesh module
contains only that shared geometry.

For isolated local runs, set `VIBE_CITY_CONFIG_DIR` to a separate preferences
directory. This keeps test notes, reading progress, and sound settings separate.

## Build and test

```sh
cargo fmt -p repo-world -p repo-city -- --check
cargo test -p repo-world -p repo-city --locked

rustup target add wasm32-unknown-unknown
# Install wasm-bindgen-cli at the wasm-bindgen version in Cargo.lock.
npm ci --prefix web
npm test --prefix web
./scripts/build-web.sh
npm exec --prefix web -- playwright install chromium
npm run test:browser --prefix web
```

To serve the browser build locally, run
`(cd web && node tests/serve.mjs 18791)` and open
[http://127.0.0.1:18791](http://127.0.0.1:18791). Browser tests run the
compiled WASM against GitHub API fixtures.

## Deploy

[GitHub Actions](../.github/workflows/ci.yml) runs formatting, browser-service
unit tests and repository-core tests on Linux with a three-minute job limit.
It does not build desktop packages or publish after a push. Build and verify
WASM and desktop packages locally from the same release commit.

The Cloudflare Worker serves static assets and the compressed WASM module at
[repo-city.meta-uber-engineer.dev](https://repo-city.meta-uber-engineer.dev/).
Run Wrangler from `web` after `scripts/build-web.sh` completes. Keep existing
Worker variables and secrets when uploading a version, then deploy that
version and compare the live assets with the tested build. The optional
manual publication workflow accepts a previously uploaded `browser-build`
Actions artifact; normal local releases use Wrangler directly.

For GitHub sign-in, register a public [GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app)
with callback `https://repo-city.meta-uber-engineer.dev/github/callback`.
Grant read access to repository Contents, Metadata, and Pull requests,
and account Starring. Leave the webhook and OAuth authorization during
installation disabled. Set `REPO_CITY_GITHUB_CLIENT_ID`,
`REPO_CITY_GITHUB_CLIENT_SECRET`, and `REPO_CITY_GITHUB_APP_SLUG` as secrets
in the `production` environment, then deploy. A private PR from a fork
requires access to the base repository and the head fork. The Worker keeps
the user token in an encrypted HttpOnly cookie with an eight-hour lifetime.

The optional `TINSINK_BROWSER_KEY` GitHub repository variable enables
production analytics. Use the public browser key for the `repo-city`
project, restricted to the production origin. Events include page views and
repository-open outcomes, timings, and whether a comparison base was
requested. Payloads exclude repository names, revisions, source, identifiers,
URL parameters, and cookies. Local builds and previews skip tracking, as do
browsers with Do Not Track or Global Privacy Control enabled.

The optional GitHub App needs **Pull requests: write** for explicit PR
comment publication. Read access still permits private drafts and GitHub
thread viewing. The Worker accepts review writes only from the site origin
with a valid user session, and checks that the comment still targets the
current PR head and an available GitHub diff range.

Browser syntax uses separately served Starry Night grammars. `web/build-syntax.mjs`
generates the complete lazy-loader catalog from the pinned package during tests
and builds. Loading a Lean file downloads its grammar and required dependencies;
the game WASM does not contain them. Filename/interpreter metadata comes from the
pinned Linguist projection in `web/syntax-language-map.json`. Ambiguous extensions
prefer common grammars and `.lean` defaults to Lean 4. These are deterministic
rules, rather than GitHub's statistical language classifier. Assets, Oniguruma
WASM, source modules and license notices are served from the game origin.

The optional web telemetry records coarse tour actions (start, complete,
skip) and a successful GitHub connection. It sends no GitHub handle,
repository name, source, search term, or comment.
