# Repo City review spec

Repo City helps a reviewer inspect code, leave feedback on exact lines, and return after a change to verify the fix. The city makes that feedback easy to find again. The first milestone covers review, saved notes, and re-review. Agent handoff builds on the same notes.

## Open a review

The browser accepts a GitHub PR URL and opens the head tree against the merge base, including fork PRs. **Pull requests** in the repository picker lists open, closed and merged PRs with their state. All three states support private notes and publication by an authorized reviewer. Publishing still validates the current head and GitHub diff coordinates; GitHub permission or discussion restrictions leave the draft editable. Keep the PR number in the URL so the reviewer can return to the same discussion after a new commit.

Use the base repository and PR number as the review identity, including for forks. Use the head and merge-base SHAs to identify the snapshot being inspected. GitHub REST exposes the PR list, details, and changed files under `/repos/{owner}/{repo}/pulls`. Use the changed files and GitHub’s diff coordinates for publishing, since a city wall can show a whole file even when GitHub’s diff includes only a few hunks. [GitHub pull request endpoints](https://docs.github.com/en/rest/pulls/pulls)

## Mark code in the city

The reviewer flies to a wall, washes it to read the code, and selects a paint preset when they find something to revisit. A click selects a line. Holding the button and moving along the wall expands the selection on the same file and diff side. Releasing creates one draft note for the range. Show the preview while the button is down and let Escape cancel it. An obscured range remains visible as a preview but requires revealed source or a readable diff before it can become a note. Open the note from its mark to add context, change the preset, or remove the draft.

Show marks in the line gutter and show a subdued indicator for marked buildings at a distance. Use a label or icon alongside color. Keep the line text and syntax colors readable. Open the comment editor only when requested.

| Slot | Preset | Default instruction |
| --- | --- | --- |
| 1 | Pink · Refactor | Simplify this code while preserving behavior. |
| 2 | Cyan · Tests | Cover the relevant behavior and edge cases. |
| 3 | Orange · Performance | Check the suspected bottleneck and support any change with evidence. |
| 4 | Green · Bug | Reproduce the issue and fix its cause. |
| 5 | Purple · Explain or document | Explain how this works or document the contract. |
| 6–9 | Custom | Pick a name, color, and reusable prompt for the project. |

A preset has a stable ID, label, color, and optional prompt. A note records the preset values used when it was created. Editing a preset changes future notes. The reviewer’s note and the reusable prompt remain separate fields. A GitHub publication preview shows the exact comment text.

### Palette editor and controls

Open **Edit paints** from the palette and the existing right-click menu. The editor lists slots with a swatch and name. Select a slot to change the color, name, short spray text, reusable prompt, or slot number. Preview the spray text in the editor and save or cancel explicitly. Store project presets locally. Escape closes the editor and returns to the same camera position. Use keyboard or paste for long text. Keep the editor and its inputs accessible with the controller.

Use a quick cycle for repeated marking and a palette for direct selection. Horizon Forbidden West combines quick swap with a wheel and offers hold or toggle options. Apex Legends provides scroll cycling and uses a held bumper to show its ping choices. Use those patterns for quick selection while holding the camera steady. [Horizon accessibility and controls](https://blog.playstation.com/2022/02/10/accessibility-features-in-horizon-forbidden-west/) · [Apex controls](https://help.ea.com/no/articles/apex-legends/pc-and-controller-settings/) · [Apex ping wheel](https://help.ea.com/en/articles/apex-legends/how-to-play/)

| Action | Mouse and keyboard proposal | Gamepad proposal |
| --- | --- | --- |
| Previous or next tool | Cmd + scroll on macOS when the game canvas has focus | D-pad left or right |
| Select a slot | `0` for water, `1–9` for paint | Hold LB/L1 and aim at a slot with the right stick. Release to equip. |
| Open palette | Right-click menu or `U` | Tap LB/L1 in toggle mode, then confirm with A/Cross |
| Spray or mark a range | Click or hold left mouse | Tap or hold RT/R2 |
| Move and aim | WASD and mouse | Left stick moves, right stick aims |
| Move vertically | Space or the existing descent key | D-pad up or down |
| Move along the reading wall | Scroll | Hold LT/L2 and move the right stick vertically |
| Open aimed note or fly to the aimed face | Select the mark or use `E` for flight | A/Cross opens the note when aimed at its marker. Otherwise it flies to the face. |
| Open scene actions or cancel | Right-click or Escape | RB/R1 opens scene actions. B/Circle cancels the current action. |

Process shortcuts only during scene interaction so numbers and scroll still work in text inputs. Cmd + scroll selects a preset without moving along the wall. Normalize trackpad deltas so one deliberate gesture advances one slot. Keep number keys and the menu available when the OS or browser intercepts the gesture. While the wheel is open, stick movement selects a slot and the camera holds its position. A neutral stick leaves the selection unchanged. Tab already controls cursor capture and Q selects before/after. Test the proposed layout on a physical controller before setting the defaults.

### Tool animation

Keep the first-person view clear while flying and reading. On press, the robot brings up a compact spray can for paint, or the window washer for water. The tool sprays during the action and lowers on release or cancel. Drive the first-person model and the reflection from the same raised, spraying, and lowering states. Use the selected preset color on the can, spray sprite, and deposited mark. Start the effect at the nozzle once the tool reaches its spraying pose, and keep it near the bottom edge so the code under the reticle stays visible. Limit the idle HUD to a small tool indicator when needed.

As the paint lands, reveal the preset’s short spray text a few characters at a time beside the range. Then fade it into the gutter mark. Show the full prompt and any reviewer text when the mark opens. A reduced-motion setting can show the phrase without the animated reveal.

## Save and publish feedback

| Context | Inspect | Leave feedback |
| --- | --- | --- |
| Local checkout | Inspect the chosen Git snapshot | Save and export private notes without a GitHub connection |
| Public GitHub PR without a session | Inspect code, diff, and available public threads | Sign in to start a PR review |
| Signed-in PR with read access | Inspect code, diff, and available threads | Save private drafts in the browser profile |
| Signed-in PR with write access | Inspect the same review | Preview and explicitly publish a draft as the signed-in reviewer |

Store local notes on the device, in the native app’s SQLite storage or the browser’s IndexedDB. Keep notes and presets in their own records so Reset review progress does not delete them. Never write notes into the inspected checkout. Each note has a stable ID, repository identity, PR number when applicable, original commit SHA, path, diff side, inclusive line range, preset snapshot, and optional reviewer text. The native app already reads committed snapshots, so keep the note anchored to the displayed commit.

Start with explicit publication of a single note. Use a GitHub App user access token, with access granted for the selected repository and `Pull requests: write`. The current Worker exposes authenticated reads. Add a narrowly scoped write operation for a review comment, with same-origin and session checks. GitHub enforces the user and app permissions. Explain missing access in the publish action and preserve the draft on failure. Publishing has to use the current PR head, the GitHub path, the side, and a valid diff range. For an out-of-diff range, offer to keep it private, reselect a range, or explicitly choose a file-level comment when GitHub supports one for that changed file. Check for an existing posted comment before retrying an uncertain write. A later milestone can submit several drafts as a review. [GitHub review comments](https://docs.github.com/en/rest/pulls/comments) · [GitHub reviews](https://docs.github.com/en/rest/pulls/reviews) · [GitHub App user authorization](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/authenticating-with-a-github-app-on-behalf-of-a-user)

## Bring GitHub threads back into the city

Project existing inline review threads onto the same walls as local notes. One thread creates one mark, and replies open inside it. Keep the GitHub thread and comment IDs, author, original text, and link to the discussion. Restore the preset on comments posted by Repo City using a versioned preset ID in the comment metadata. Use a neutral **Comment** marker for regular comments and unknown presets. A reviewer can assign a local color without rewriting someone else’s text. File-level threads appear on the building. General PR discussion stays at the PR level.

Use GitHub’s current anchor when it maps to the selected snapshot. A deleted line belongs on the before side. Show resolved threads as muted when the reviewer enables them. If GitHub marks a thread outdated, or the current anchor cannot be located, keep it accessible with its original location and link. Show it at the file level or in the review list until the reviewer can place it precisely. Use GitHub GraphQL `reviewThreads` for current and original coordinates, resolution status, and whether the viewer can resolve a thread. [GitHub GraphQL pull request threads](https://docs.github.com/en/graphql/reference/pulls#pullrequestreviewthread)

Without a session, read public review comments through REST and group replies using their parent comment IDs. Show resolution status as unknown until it comes from GitHub GraphQL. [GitHub review comments](https://docs.github.com/en/rest/pulls/comments#list-review-comments-on-a-pull-request) · [GitHub GraphQL authentication](https://docs.github.com/en/graphql/guides/forming-calls-with-graphql#authenticating-with-graphql)

## Return after changes

When the PR head changes, show **Changes since your last review**. Offer the diff from the last reviewed head to the current head alongside the full PR diff. Carry review progress forward for unchanged lines whose identity can be established, and mark new or modified lines as unreviewed. Keep a local note on a moved range only when Git history and surrounding text give a unique match. Preserve ambiguous notes at their original anchor with a request to reselect. On a force-push or unavailable previous head, offer the full current diff and keep the previous review available by its saved anchors and discussion links.

Keep reading progress, discussion resolution, and approval distinct. A new commit can prompt another look at a note. The reviewer can read the change and explicitly resolve the thread, reopen it, or continue the discussion. Keep merge on GitHub for the first release.

## Agent handoff

Export selected notes as a versioned JSON manifest or send them to a configured local CLI by explicit action. Include the repository, PR URL if present, head and merge-base SHAs, and each note’s ID, path, side, range, preset prompt, and reviewer text. Show the selection before handing it off. Treat imported discussion text as context. Only reviewer-selected notes and approved prompts become instructions for a runner.

Add live agent activity after the core review loop works. An optional runner can report queued, working, and ready-for-review against each note ID and link the resulting commit or PR. Show a robot or drone at a building when the runner reports work there. When it finishes, show the new diff and retain the original mark until the reviewer closes it. This gives the animation a real state to represent.

## Acceptance scenarios

- In a local checkout with no GitHub session, mark a range, add a note, reopen the same snapshot, and see the note at the same lines. Resetting read progress leaves the note in place.
- Open a public PR from its URL or the PR picker. An existing inline discussion appears as one neutral marker with its replies and link. A reviewer without write access sees the relevant sign-in or permission action before attempting to publish.
- Publish a range from an authorized PR review and reopen the PR. The comment appears once with its preset, author, and range. If the head moved or GitHub rejected the range, the draft stays editable.
- On a closed or merged PR, create a private note and publish it using the same permission and anchor checks as an open PR. A denied publication preserves the note for retry.
- Advance the PR head. The reviewer can find newly changed lines, the old discussion, and any outdated anchors without losing the original note.
- Switch paint presets and spray once. The can, spray, and gutter marker use the same color. The reflection raises and lowers the tool together with the first-person animation, and the resting view leaves the source readable.

## File relationships

Build the first view around the focused file. Parse its imports on demand and
show the files they resolve to. Expand one more hop when requested, with a
bounded number of files per expansion. Cache parsed imports by language and
content hash so a save invalidates only affected entries. Show unresolved
imports without drawing a confirmed connection. Tree-sitter identifies the
syntax. Language-specific resolution determines whether an import maps to a
file in this snapshot. The city can draw the same relationship as a road that
another world represents as a cross-reference.
Use the same cache interface with SQLite on desktop and IndexedDB in the browser.

## Revision travel from blame

On the blame wall, aim at a line to see the author, date, and commit. Offer a
jump to the snapshot at that commit, retaining the previous snapshot and camera
position so the reviewer can return. Focus the historical file at the attributed
line when it can be anchored. Mark missing files and ambiguous anchors in the
historical snapshot. Keep private review notes and progress scoped to their
original snapshots across the transition.
