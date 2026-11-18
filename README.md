# Vibe City

Vibe City opens a local Git checkout as a 3D city. Directories become districts;
files become buildings with source, diff, and blame walls. Review uncommitted
changes without switching branches, mark lines with reusable review paints,
and share local notes and your current code context with an MCP client.

![VibePad shows sound and reading settings inside the handheld screen](docs/vibe-city.png)

The browser version is [Repo City](https://repo-city.meta-uber-engineer.dev/).
The repositories share history; [Repo City](https://github.com/bobishh/repo-city)
is the upstream for shared code.

## Open a repository

### On your Mac

The desktop app is called **Vibe City**. Build it with Rust and open the resulting bundle.

```sh
git clone https://github.com/bobishh/vibe-city.git
cd vibe-city
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
| Preview Markdown or an image | Aim at its building and press **P**. VibePad renders Markdown and PNG, JPEG, or WebP images. |
| Fly | **WASD**, **Space** and **Ctrl** for altitude, **Shift** to boost |
| Capture or release the cursor | Click the scene to capture. **Esc** releases. |
| Use the actions menu | Right-click to open or close it. Scroll or use the arrow keys to highlight an action. Click or press **Enter** to select it. |
| See the city from above | Press **M** for the district, then the whole project. On the project map, click a district first, then a building. The cursor stays free in map views. **M** returns to your original view after a district jump. Fly freely after each move. |
| Return to a building | **Ctrl+O** goes back in jump history and **Ctrl+I** goes forward. |
| Browse changes | **Tab** opens and puts away VibePad. **Viewed** limits the list to changes you visited. Select a file to fly there. **Shift+Tab** moves focus inside VibePad. |
| Approach a wall | Far away, left-click a building to fly to its nearest covered lines or the top of its source wall. Press **E** to fly to the aimed face. Scroll to move along the wall. |
| Wash or paint lines | **0** equips water. Click close to a source wall to start or stop water. Code appears while you aim at the block. In VibePad **Settings**, choose **Immediate** to save the reveal right away or **Delayed** to wait for the reading estimate. **1–9** equip paints; drag from one readable line to another on the same side of the file. |
| Edit paints and notes | **U** opens paints; **⌘ + scroll** cycles tools on Mac. **J** opens the note you are aiming at or the review list. |
| Find text on the current wall | **/** or **Find in file** in the actions menu. **Enter** keeps results; **N** and **Shift+N** visit matches. |
| Turn to the diff or blame wall | **G** for diff, **B** for blame. Sign in with GitHub for line attribution in the browser. |
| Follow a file’s changes | **C** starts the review. Scroll up or press **W** for the next stop. Scroll down or press **S** for the previous stop. **Q** returns to the diff wall. |
| Open controls | **H** opens VibePad Help. Choose **Start tour** to practice flying and washing source. |

Delayed mode starts with a minimum of 3 seconds and a maximum of 6 seconds.
Clear either setting to remove that bound. Repo City estimates reading time
from the amount of code using [Brysbaert’s English nonfiction reading rate](https://doi.org/10.1016/j.jml.2019.104047)
as an approximation. The Mac stores `mode`, `min_seconds`, and `max_seconds`
in `~/Library/Application Support/Repo City/reading.json`. A `null` bound
has no limit.

Reach the end of a Markdown preview to reveal the file's source wall.
Repo City saves revealed lines and reviewed files in your browser or on your
Mac. The same repository and comparison restore those marks. Press **V** to
toggle the selected file’s review mark. To start over on the open comparison,
choose **Reset progress** in the browser’s repository dialog or the Mac
**Files** menu.

The city shows the percentage of files visited and changed files whose diff
you opened. In the browser, a pull request or branch update keeps visits to
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

The gear at the right of VibePad's navigation opens **Settings**. Set the water
reading delay there, mute the city, adjust volume, or turn robot speech off.
Wind, spray, construction, and occasional sirens are synthesized locally.
On macOS the robot uses the installed system voice to quote short, readable
fragments from the source wall. The estimated reading position is approximate;
it does not track your eyes. Speech stops when the code context changes.

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
**Mark viewed on GitHub 👀** adds the reaction from your account and restores
the thread's usual color when you have marked its mentions viewed.

For local stack traces, draft editing, builds, and deployment, see
[Development](docs/development.md).

### Share local review notes with an MCP client

Run the native binary with `--notes-mcp --repo /absolute/path/to/checkout`
as a stdio MCP server. `list_notes` reads local worktree notes for that
checkout. `mark_note` sets a note to `unread`, `read`, or `addressed`.
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
