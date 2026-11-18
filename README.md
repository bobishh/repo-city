# Repo City

[Repo City](https://repo-city.meta-uber-engineer.dev/) opens GitHub repositories
and pull requests as a 3D city in your browser. **Vibe City** opens local Git
checkouts on desktop, including live worktrees, an integrated terminal and
local review notes shared through MCP.

Directories become districts; files become buildings with source, diff, and
blame walls. Both applications use the same Rust engine in this workspace.
Licensed under **MIT OR Apache-2.0**; choose either license.

![VibePad renders sound and reading settings with Bevy UI](docs/vibe-city.png)

## Workspace

- `crates/repository` owns repository snapshots, comparisons and source analysis (`repo-world`).
- `crates/game/src/repo_city/lib.rs` exposes the shared engine as `repo_city_core`.
- `repo-city` is the browser entry point; `vibe-city` is the desktop entry point.
- `web` supplies browser storage, GitHub access and speech through a service bridge.
  Bevy renders the tablet screen and widgets on desktop and in the browser.

Use one release commit for the browser deployment and desktop packages.
Desktop adapters stay native-only; browser builds do not contain the local terminal.
The former `bobishh/vibe-city` repository is superseded by this workspace.

## Open a repository

### On your Mac

The desktop app is called **Vibe City**. Build it with Rust and open the resulting bundle.

```sh
git clone https://github.com/bobishh/repo-city.git
cd repo-city
./scripts/build-macos.sh
open "dist/Vibe City.app"
```

Click the robot **Vibe City** logo and choose **Open local repository…**, or
press **Cmd+O**. The same menu opens **Revisions…**, **GitHub…**, and file search.
**Revisions…** lists branches, tags, and recent commits. You can view a commit
without switching your working tree. For terminal use, `./scripts/run-native.sh`
opens Repo City's checkout by default and accepts another checkout path as its
first argument.

Choose **Revisions… → Live worktree** to compare the working directory with
`HEAD`. Saves update changed buildings, and new non-ignored files appear in
the city. An update appears briefly above the city controls. Click it or press
**L** to fly to the last changed file. Press **Tab** to open the changed files
tablet and see earlier changes. Press **F** to show only changed buildings and
**G** at a changed file to read its diff. From the command line,
run `./scripts/run-native.sh /path/to/checkout --live`.
The script runs the Rust development build. The watcher updates the city when
the inspected checkout changes. Restart the program to load changes to Repo
City's own Rust code. The desktop tablet uses watcher times or file modification
times in a worktree and commit times for Git comparisons. The browser uses
GitHub commit history where available.

Choose **GitHub…** from the logo menu to find a repository or paste a GitHub repository or pull
request URL. **Sign in with GitHub** opens the verification page. Click the
device code to copy it, then paste it in the browser. Once signed in, the picker
suggests your repositories, forks, and starred repositories as you type.
Select a repository to choose a revision and comparison base or list its pull
requests.
The app uses Git to fetch snapshots and the GitHub API for review comments.

In large repositories the city loads nearby buildings as you fly. File search
still covers the entire comparison.

### On Linux or Windows

Both desktop ports require Git on `PATH` and a graphics driver supported by
Bevy: Vulkan on Linux, DirectX 12 or Vulkan on Windows. GitHub access uses the
app's sign-in flow; `gh` is not required.
On Linux and Windows, **Open local repository…** opens a path field inside
the app. Enter the checkout directory, then choose **Open**. You can also pass
`--repo /path/to/checkout --live` from a terminal.

On Debian or Ubuntu, install the native build dependencies before building:

```sh
sudo apt install build-essential pkg-config libasound2-dev libudev-dev \
  libfontconfig-dev libwayland-dev libxkbcommon-dev libx11-dev libxkbcommon-x11-0
./scripts/build-linux.sh
```

The script creates `dist/VibeCity-<target>.tar.gz`. Extract it and launch
`vibe-city`; the package lists its runtime dependencies. Build with Rust 1.98
or newer. On Windows, install Rust with the MSVC toolchain, Visual Studio C++
Build Tools, and Git for Windows, then run:

```powershell
./scripts/build-windows.ps1
```

Extract the resulting `dist/VibeCity-x86_64-pc-windows-msvc.zip` and launch
`VibeCity.exe`. The Windows script accepts `-Target`, `-BuildProfile`, and
`-OutputDirectory`. The Linux script accepts `CARGO_BUILD_TARGET`,
`VIBE_CITY_BUILD_PROFILE`, and `VIBE_CITY_OUTPUT_DIR`; cross builds need a
matching C compiler and platform libraries. Both packages include license
notices. These build scripts do not add CI jobs.

Settings and local notes use `%APPDATA%/Repo City` on Windows and
`${XDG_CONFIG_HOME:-~/.config}/Repo City` on Linux. System voice speech and
secure sign-in restoration currently use macOS services; Linux and Windows
keep the GitHub session for the current app run.

### In your browser

Open [Repo City](https://repo-city.meta-uber-engineer.dev/) in a browser with
WebGPU. Paste an `owner/repository` name, a GitHub repository URL, or a GitHub
pull request URL. For a repository, choose a branch, tag, or commit from the
suggestions. Pick a comparison base to review a branch from its merge base.
With no base specified, Repo City compares the revision with its first parent.
A pull request opens its head tree and highlights changed lines in place. The
diff wall also shows deleted lines. Paste a pull request URL or choose **Pull
requests** after selecting its repository. Closed and merged pull requests
show their historical changes and comments. Publishing review notes requires
an open pull request.
Press **F** or choose **Only changes** from the right-click menu to lower
untouched buildings below the street and filter the file picker. Press **F**
again or choose **Show all files** to restore the full city.

Use **Sign in with GitHub** below the search to find your repositories, stars,
and forks from the repository field. Under your GitHub handle, use **Choose
private repositories** to grant read access to selected repositories. Use
**Default branch** or **Compare…** to find branches and tags or search commit messages
on the default branch. Click the **Repo City** logo to reopen the picker.
The first city visit offers a controls tour. Open **H** to take it again.

## Controls

| Action | Control |
| --- | --- |
| Find a file by name or path | **P** in the browser, **⌘P** on Mac |
| Preview a document or image | Aim at its building and press **P**. VibePad renders Markdown, plain text and code, and PNG, JPEG, or WebP images. |
| Fly | **WASD**, **Space** and **Ctrl** for altitude, **Shift** to boost |
| Capture or release the cursor | Click the scene to capture. **Esc** releases. |
| Use the actions menu | Right-click to open or close it. Scroll or use the arrow keys to highlight an action. Click or press **Enter** to select it. |
| See the city from above | Press **M** for the district, then the whole project. On the project map, click a district first, then a building. The cursor stays free in map views. **M** returns to your original view after a district jump. Fly freely after each move. |
| Return to a building | **Ctrl+O** goes back in jump history and **Ctrl+I** goes forward. |
| Browse changes | **Tab** opens and puts away VibePad. **Viewed** limits the list to changes you visited. Select a file to fly there. **Ctrl+Tab** moves to the next control; **Shift+Tab** moves to the previous one. |
| Approach a wall | Far away, left-click a building to fly to its nearest covered lines or the top of its source wall. Press **E** to fly to the aimed face. Scroll to move along the wall. |
| Wash or paint lines | **0** equips water. Click close to a source wall to start or stop water. Code appears while you aim at the block. In VibePad **Settings**, choose **Immediate** to save the reveal right away or **Delayed** to wait for the reading estimate. **1–9** equip paints; drag from one readable line to another on the same side of the file. |
| Edit paints and notes | **U** opens paints; **⌘ + scroll** cycles tools on Mac. **J** opens the note you are aiming at or the review list. |
| Find text on the current wall | **/** or **Find in file** in the actions menu. **Enter** keeps results; **N** and **Shift+N** visit matches. |
| Turn to the diff or blame wall | **G** for diff, **B** for blame. Up close, **Left/Right** cycles source, diff and blame. Sign in with GitHub for line attribution in the browser. |
| Follow a file’s changes | **C** starts the review. Scroll up or press **W** for the next stop. Scroll down or press **S** for the previous stop. **Q** returns to the diff wall. |
| Fold a code block | **Z** collapses the aimed function; **X** expands it. Fixed-line blocks are used when syntax boundaries are unavailable. |
| Open controls | **H** opens and puts away VibePad Help. Choose **Start tour** to practice flying and washing source. |

**Reading block** chooses whole functions where syntax boundaries are available,
or fixed-line blocks. **Lines per wash block** sets the fallback/fixed block size
(default: 6). Set it to 1 to clear individual rows. **Auto-advance** smoothly
scrolls downward to the next unread block while water remains on. This setting is independent of the
delay bounds. Wrapped continuations count as displayed rows; the original
source line is saved as read only after all its continuations are cleared.
Reading a close diff, blame wall, or hunk stop uses the same timer and reveals only its visible source lines on the correct comparison side. Flying past or looking away does not complete a block.
While washing, code is visible immediately. Leaving an unfinished block covers
it again. A ring around the tool-colored reticle shrinks as the delay expires.

Delayed mode estimates seconds as `effective non-whitespace characters × 60 / (238 × 4.6)`
for the selected block. Consecutive copies of one character count once, so a
decorative separator does not create a long delay. This repetition discount is
an interface heuristic, separate from the cited prose reading rate. A minimum or maximum delay clamps that result; clear
either field to remove that bound. The initial bounds remain 3 and 6 seconds.
The length-adjusted [Brysbaert English nonfiction rate](https://doi.org/10.1016/j.jml.2019.104047)
is an approximation, not a measured code-comprehension speed. Six rows is a
configurable preset, not a research-backed optimum. The Mac stores `mode`,
`block_lines`, `block_kind`, `auto_advance`, `min_seconds`, and `max_seconds` in
`~/Library/Application Support/Repo City/reading.json`. A `null` bound has no limit.

Reach the end of a Markdown or text preview to reveal the file's source wall.
Repo City saves revealed lines and reviewed files in your browser or on your
Mac. The same repository and comparison restore those marks. Press **V** to
toggle the selected file’s review mark. To start over on the open comparison,
choose **Reset progress** in the browser’s repository dialog or the Mac
**Files** menu.

The city shows counts and progress bars for visited files and changed files
whose diff you opened. In the browser, a pull request or branch update keeps visits to
unchanged file contents and diffs.

## Follow file connections

Open **Tab → Connections** while focused on a file. VibePad lists its imports
and resolved local targets; select a target to fly to its definition line.
When only the module is known, the target says **module start**. Roads connect
resolved files in the visible city. Following a link restores hidden buildings
if **Only changes** would hide the destination.

The native resolver supports static JavaScript/TypeScript imports and Rust
modules and qualified imports. External packages and ambiguous targets remain
listed with an explanation. This is syntax-based navigation, not a compiler
call graph. Parsing runs in a background worker and its in-memory cache uses
language and content identity.

## Sound and preferences

The gear at the right of VibePad's navigation opens **Settings**. Valid changes save automatically; invalid fields show a reason and keep the last valid settings. Set the water
reading delay there, mute the city, adjust volume, or turn robot speech off.
Sound and narration start disabled. Wind, spray, construction, and occasional sirens are synthesized locally when enabled.
Choose **Radio** in Sound settings for **Space trance** or **Techno**. It synthesizes
drums, bass, chords, arpeggios and echo locally, with evolving phrases and no external
music service. Radio starts off and works independently of city ambience. Both become
quieter during narration.
The browser starts its AudioWorklet after the first click or keypress. Preferences
persist on the device; opening the repository picker or hiding the page silences it.
The robot reads the current readable block from start to end, in source order.
Choose an installed local voice in Settings; the default prefers Daniel.
Water is muted and other city sounds are quieter during speech. Leaving the
block lets the current utterance finish; mute, opening a menu, or hiding the
application stops speech. macOS uses system voices; desktop speech is currently
unavailable on Linux and Windows. Browser voice availability depends on installed
local voices.

## Git history and terminal

Open **Tab → Git** to browse commits and inspect their patches. Pick **Before**
and **After**, then **Compare selected commits** to open those exact snapshots
without checking out either commit. The browser uses GitHub history; it does not
expose a local terminal.

On desktop, **Tab → Terminal** opens a persistent shell in the current checkout.
It uses Alacritty's terminal emulator and a real PTY, including cursor movement,
alternate screens, UTF-8, resizing, clipboard paste and Ctrl+C. **Ctrl+Tab** puts
away the tablet while the shell keeps running; plain Tab and Esc go to the shell.
**F6** switches keyboard focus between the shell and tablet controls.
Shell commands run in the inspected repository and may change it. Git history
and comparison controls only read it. If Git is missing, install it and restart
the app; the tablet shows installation guidance rather than closing the app.

Without a mouse, **arrows** turn the camera in flight. Near a wall, **Up/Down**
scrolls and **Left/Right** changes its face; hold **Shift** for free look. **K**
uses the selected tool: tap to toggle water or hold to paint.

**Alt+0** opens repository selection. **Shift+F10** opens the actions menu.

In VibePad, **Alt+1…6** selects a tab in its displayed order. Arrow keys move
focus, **Enter/Space** activates a control, and **Esc** goes back or closes the
panel. **Tab** puts VibePad away; **Shift+Tab** moves focus. Text fields and the
terminal keep their editing keys. **Cmd/Ctrl+P** opens Find in VibePad.

## Touch controls

The browser shows separate movement and look sticks and icon buttons on devices with a coarse
pointer. Drag the city to look around. Near a readable wall, swipe vertically to
scroll and sideways to cycle source, diff, and blame. Moving the joystick
leaves the wall.
Use Up, Down and Fast to fly; altitude buttons hide while reading. **Pad** opens
VibePad, and the top-right cross puts it away. Controls reset when a menu opens,
the page loses focus, or a touch is cancelled. A WebGPU-capable browser and GPU
remain required. Landscape provides more reading space.

## Leave review notes

Wash the source or use the readable diff wall. Pick a paint with **U** and
drag over lines to color them and save a draft with that paint's reusable
prompt. Each draft keeps its original file, revision, and line range.
Change a paint's name, color, spray text, or reusable prompt in **Paints**.

The Mac app saves notes and palettes in SQLite. Press **J** while aiming at a
marked line to open its comment in VibePad. Away from a mark, **J** opens all
comments on the current comparison. **All comments** returns to that list.
Use **Edit** on a note to add details, then **Save**. Worktree notes are
saved locally and available through MCP immediately; **Publish** applies only
to GitHub pull requests. **Copy** copies a note without exporting a file.
An empty paint slot equips a plain note without a required prompt.
Comment previews render Markdown in VibePad.
When you open a GitHub pull request, **J** also lists its review comments.
Choose **Fly to lines** to see an anchored comment on its building, or
**Open on GitHub** to read the full thread. In the **M** overview, gold pins
mark buildings with comments on the current comparison.
In an open pull request, **Publish all** sends the current drafts to GitHub
with write access. Turn on **Auto-publish** to send new marks as you make them.
This setting turns off when you open another comparison. GitHub sign-in uses
the macOS Keychain to restore the session.

In the browser, sign in with GitHub to save drafts for a pull request in
IndexedDB. Press **J** while aiming at a note to open it. Existing public
review threads are readable without signing in.
Choose a local paint for a GitHub thread from its detail panel. GitHub still
shows the original comment. Signed-in reviewers can show resolved threads from
the review list.

The draft starts with the paint's reusable prompt. **Publish to GitHub**
sends that comment in one action. Use **Edit comment** to adapt it or
**Write a comment** to start blank. Publication requires repository access
and **Pull requests: write** for the Repo City GitHub App. Select **Suggest
replacement** on the current side of a pull request to preview and publish
replacement code. GitHub accepts suggestions within one diff hunk. If the
PR moves or GitHub rejects the range, the draft stays local. Resetting
reading progress leaves notes and paints in place.

On a signed-in review, unresolved inline comments that mention your GitHub
handle are marked in gold until you view them. Open **Review notes** and
choose **Fly to next mention**. Click the city to capture the cursor and
press **J** once the camera reaches the marked lines.
**Mark as viewed** adds an eyes reaction from your GitHub account, without changing
the comment text, and restores
the thread's usual color when you have marked its mentions viewed. Local checkout
notes use the same button to save their viewed status for MCP without a GitHub reaction.

For local stack traces, draft editing, builds, and deployment, see
[Development](docs/development.md).

### Share local review notes with an MCP client

Run the native binary with `--notes-mcp --repo /absolute/path/to/checkout`
as a stdio MCP server. `list_notes` reads private local notes from both commits and live worktrees in that
checkout. Each note includes its revision and comparison base. `mark_note` sets a note to `unread`, `read`, or `addressed`.
`get_focus` returns the current file, wall side, camera, and the source lines
that are visible and readable. `list_notes` includes the same focus context.
Context expires after six seconds; diff and blame currently report their side
without a visible line range. Replace the MCP executable and restart the server
after updating the app.
An addressed note stays in the review list and its paint disappears from
the city. Local note tools do not publish to GitHub.

## License

Choose the [MIT license](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE).
See [third-party notices](THIRD_PARTY.md) for separately licensed material.
