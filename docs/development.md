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
JavaScript, TypeScript, Ruby, Elixir, Elm, and TLA⁺. The browser loads the complete
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

The desktop Codex integration follows Ecky's app-server configuration at
`9ea94d7`. It requests Codex's automatic review for workspace sandbox increases
and preapproves the checkout-bound Repo City MCP tools. Manual requests can still
be shown by the provider. User acceptance of MCP flight proposals is handled
separately in the city.

## Build and test

```sh
cargo fmt -p repo-world -p repo-city -- --check
cargo test -p repo-world -p repo-city --locked

rustup target add wasm32-unknown-unknown
# Install wasm-bindgen-cli at the wasm-bindgen version in Cargo.lock.
npm ci --prefix web
npm test --prefix web
(cd server && mix deps.get && mix test)
./scripts/build-web.sh
npm exec --prefix web -- playwright install chromium
npm run test:browser --prefix web
```

To serve the browser build locally, run
`(cd server && PHX_SERVER=true BIND_ADDRESS=127.0.0.1 PORT=18791 mix run --no-halt)` and open
[http://127.0.0.1:18791](http://127.0.0.1:18791). Browser tests run the
compiled WASM against GitHub API fixtures.

## Deploy

### Review room service

Phoenix owns one process per room on the shared Hetzner VPS. The browser uses a
WebSocket on the application origin and desktop posts events and polls the same
room over HTTP. `GET /party/config` reports whether room creation and joining are
enabled. Set `REVIEW_PARTIES_ENABLED` to `true` in Kamal or as a GitHub Actions
production environment variable.

Room IDs identify individual sessions. The comparison ID is a SHA-256 hash of
the GitHub repository ID, base commit, head commit, and layout version. Invites
bind to a verified GitHub account. Their secret is in the URL fragment so it
does not travel in HTTP requests for the page. Phoenix keeps only hashes of
invite and room secrets. Each participant verifies and loads repository contents
using their own GitHub authorization.

Membership renews GitHub access every five minutes. Rooms last at most 24 hours
and the host has a 60-second reconnect window. A backend restart ends active
rooms. The server holds presence, line cues, the most recent 32 chat messages,
and the host's current sketch in memory. It discards directed
SDP and ICE messages after delivery or 30 seconds. Audio travels over WebRTC.

Kamal runs the TURN relay as the `turn` accessory on the VPS. Set
`REVIEW_PARTY_TURN_SECRET` as a deployment secret with at least 32 random bytes
encoded as lowercase hex. The app and relay share this secret. Phoenix uses it
to issue one-hour TURN credentials to admitted members and refreshes them at
membership renewal. The relay listens on port 3478 (UDP and TCP) and uses UDP
ports 49160–49223 for allocations. Allow those ports on the VPS public IPv4
interface. Use `kamal accessory reboot turn` to rotate the relay configuration
and redeploy the app with the same secret.

`REVIEW_PARTY_ICE_SERVERS` optionally supplies a JSON array of additional ICE
servers. For example, use `[{"urls":["stun:stun.example:3478"]}]` for a separate
STUN service.

`REVIEW_PARTY_ACTIVE_ROOMS` (default 256), `REVIEW_PARTY_ROOM_BYTES` (131072),
`REVIEW_PARTY_MESSAGES_PER_SECOND` (120), and
`REVIEW_PARTY_PENDING_INVITES` (256) protect service resources. These safeguards
are configurable in the Phoenix process environment and do not represent
measured participant capacity. Phoenix emits request duration, action, and
status through the `[:repo_city, :party, :request]` telemetry event.

Run the room protocol checks with `(cd server && mix test test/review_party_test.exs)`.
After building web assets, run the browser room fixture with
`REPO_CITY_TEST_PARTY=1 npm run test:browser --prefix web -- review-party.spec.mjs`.
With a running TURN relay, pass its `REVIEW_PARTY_TURN_URLS` and matching secret
through the environment and set `REPO_CITY_TEST_RELAY=1` to require relay-only
audio in the Phoenix browser test.

For isolated native development, `VIBE_CITY_PARTY_SERVICE` may select a
loopback HTTP origin with an unused port. Normal clients use the hosted Repo
City HTTPS origin. Room tokens authorize only their member and room; GitHub
tokens are sent only to the service for verified create/join/renew operations.

### Application deployment

[GitHub Actions](../.github/workflows/ci.yml) runs formatting, browser-service
unit tests and repository-core tests on Linux with a three-minute job limit.
It does not build desktop packages or publish after a push. Build and verify
WASM and desktop packages locally from the same release commit.

[Repo City Web](https://repo-city.meta-uber-engineer.dev/) uses the Phoenix
application in `server/` behind the shared Hetzner `kamal-proxy`. It serves static assets,
precompressed WASM, `/github/`, and `/party/` from the same origin. The [manual release workflow](../.github/workflows/release.yml) builds
and publishes `ghcr.io/bobishh/repo-city:<commit>`, then deploys that image
through Kamal. `use_published_image` reuses a verified image tagged with the
same commit. The VPS checks run against the deployed container. A normal
release also checks the public origin; select `preflight` during a DNS migration.

Set `DEPLOY_SERVER_IP` as a production environment variable and configure
`DEPLOY_SSH_PRIVATE_KEY` and `DEPLOY_SSH_KNOWN_HOSTS` from the shared VPS setup.
GitHub authorizes the workflow's GHCR reads and writes. Kamal uses automatic
Let's Encrypt TLS by default. To verify the current hostname before changing
its A record, set `REPO_CITY_TLS_CERTIFICATE` and `REPO_CITY_TLS_PRIVATE_KEY`
as production secrets with a matching publicly trusted certificate and PEM key.
Use the automatic certificate after the hostname points to the VPS.

For a cutover, check HTTPS directly against the VPS with `curl --resolve`
using the public hostname. Preserve the Worker and its deployed version.
Detach the Worker custom domain and create a DNS-only A record for the VPS.
To roll back, remove that A record and attach the same custom domain to the
preserved `repo-city` Worker. Keep the existing GitHub App configuration and
OAuth secret on both hosts throughout the switch.

For GitHub sign-in, register a public [GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app)
with callback `https://repo-city.meta-uber-engineer.dev/github/callback`.
Grant read access to repository Contents, Metadata, and Pull requests,
and account Starring. Leave the webhook and OAuth authorization during
installation disabled. Set `REPO_CITY_GITHUB_CLIENT_ID`,
`REPO_CITY_GITHUB_CLIENT_SECRET`, and `REPO_CITY_GITHUB_APP_SLUG` as secrets
in the `production` environment, then deploy. A private PR from a fork
requires access to the base repository and the head fork. The server keeps
the user token in an encrypted HttpOnly cookie with an eight-hour lifetime.
Reuse the current OAuth secret when changing hosts to preserve existing sessions.
Phoenix uses `PUBLIC_ORIGIN` for callback URLs and origin checks. Its session
and review contracts follow `web/github-auth.mjs` and `web/pr-review.mjs`
at `306a6c5`.

The VPS shares an in-memory cache for public Git trees and blobs under
`/github/public/`. It fetches those objects anonymously and keys them by
repository, object ID and tree representation. Concurrent reads of the same
key share a request. Reuse requires a public conditional response from GitHub.
If validation fails, clients fall back to the ordinary GitHub read path.
Private objects, credentials, account lists, and review data stay outside this cache.

A standard build serves the original HTML and has no browser collector. An
operator can set the production `REPO_CITY_BROWSER_SCRIPT` secret to enable an
external browser script at runtime. The JSON value takes an HTTPS `src` and an
optional `data` object of string attributes, for example
`{"src":"https://collector.example/client.js","data":{"project":"example"}}`.
Kamal passes it to Phoenix as `BROWSER_SCRIPT`. Phoenix adds the script to the
served index. Use only browser-scoped values in `data`, which visitors can read.
Clear the deployment setting and redeploy to disable the script.

The optional GitHub App needs **Pull requests: write** for explicit PR
comment publication. Read access still permits private drafts and GitHub
thread viewing. The server accepts review writes only from the site origin
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
