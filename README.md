# Repo City

Repo City opens a GitHub repository or pull request as a 3D city in your browser. Directories
form districts. Each file has walls for its source, diff, and blame. Open a
GitHub pull request to see what it changes in the context of the whole codebase.

[Explore Repo City’s own code in your browser](https://repo-city.meta-uber-engineer.dev/?repo=bobishh%2Frepo-city&ref=4ad0303336c2acf0ea785c596dbc0b5d53d55bfe&base=a035f34156d434e22142a6104b812f3bb6f13b36).
The demo opens at `crates/repository/src/syntax.rs`. Press **G** to see its
diff, or press **P** to find another file.

![The diff wall of syntax.rs, with syntax highlighting and neighboring file buildings visible](docs/repo-city-diff.webp)

The Mac application lives in the separate [Vibe City repository](https://github.com/bobishh/vibe-city).
It also supports local worktrees.

## Open a repository

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

### On your Mac

The desktop application is called **Vibe City** and is developed separately.

## Controls

| Action | Control |
| --- | --- |
| Find a file by name or path | **P** |
| Preview a Markdown file | Aim at its building and press **P** to open the formatted document in VibePad. |
| Fly | **WASD**, **Space** and **Ctrl** for altitude, **Shift** to boost |
| Capture or release the cursor | Click the scene to capture. **Esc** releases. |
| Use the actions menu | Right-click to open or close it. Scroll or use the arrow keys to highlight an action. Click or press **Enter** to select it. |
| See the city from above | Press **M** to cycle through the district, the project, and your original view. The cursor stays free in overview. Point at a district in the project map to see its path, then click to approach its files. Click a building in the district view to read its code. After entering a district from the project map, **M** returns to your original view. You can fly freely after each move. |
| Return to a building | **Ctrl+O** goes back in jump history and **Ctrl+I** goes forward. |
| Browse changes | **Tab** opens and puts away VibePad. **Viewed** limits the list to changes you visited. Select a file to fly there. **Shift+Tab** moves focus inside VibePad. |
| Approach a wall | Far away, left-click a building to fly to its nearest covered lines or the top of its source wall. Press **E** to fly to the aimed face. Scroll to move along the wall. |
| Wash or paint lines | **0** equips water. Click close to a source wall to start or stop water. Code appears while you aim at the block. In VibePad **Settings**, choose **Immediate** to save the reveal right away or **Delayed** to wait for the reading estimate. **1–9** equip paints; drag from one readable line to another on the same side of the file. |
| Edit paints and notes | **U** opens paints. **1–9** select a paint. **J** opens the note you are aiming at or the review list. |
| Find text on the current wall | **/** or **Find in file** in the actions menu. **Enter** keeps results; **N** and **Shift+N** visit matches. |
| Turn to the diff or blame wall | **G** for diff, **B** for blame. Sign in with GitHub for line attribution in the browser. |
| Follow a file’s changes | **C** starts the review. Scroll up or press **W** for the next stop. Scroll down or press **S** for the previous stop. **Q** returns to the diff wall. |
| Open controls | **H** opens VibePad Help. Choose **Start tour** to practice flying and washing source. |

Delayed mode starts with a minimum of 3 seconds and a maximum of 6 seconds.
Clear either setting to remove that bound. Repo City estimates reading time
from the amount of code using [Brysbaert’s English nonfiction reading rate](https://doi.org/10.1016/j.jml.2019.104047)
as an approximation. Settings are saved in your browser.

Reach the end of a Markdown preview to reveal the file's source wall.
Repo City saves revealed lines and reviewed files in your browser. The same repository and comparison restore those marks. Press **V** to
toggle the selected file’s review mark. To start over on the open comparison,
choose **Reset progress** in the repository dialog.

The city shows the percentage of files visited and changed files whose diff
you opened. In the browser, a pull request or branch update keeps visits to
unchanged file contents and diffs.

## Leave review notes

Wash the source or use the readable diff wall. Pick a paint with **U** and
drag over lines to color them and save a draft with that paint's reusable
prompt. Each draft keeps its original file, revision, and line range.
Change a paint's name, color, spray text, or reusable prompt in **Paints**.

The Mac app saves notes and palettes in SQLite. Press **J** while aiming at a
marked line to open its comment in VibePad. Away from a mark, **J** opens all
comments on the current comparison. **All comments** returns to that list.
Use **Edit** to add details or export a note.
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
An addressed note stays in the review list and its paint disappears from
the city. Local note tools do not publish to GitHub.

## License

Choose the [MIT license](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE).
See [third-party notices](THIRD_PARTY.md) for separately licensed material.
