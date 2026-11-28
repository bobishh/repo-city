# World set contract

A world set supplies the scene, locomotion, input presentation and physical
appearance of Repo City's tools. It receives the current repository comparison
and presents its operations. The repository, review state and services have
the same identities in every world.

Code City is the default. Repo Polis presents files as scrolls in a library.
Repo Reef places file stations on a dark ultramarine seabed. An engineer
pilots a bathyscaphe with twin headlamps among seafoam kelp and lilac rock.
The rear mirror shows the engineer as you approach.
Desktop accepts `--world-set ID`, a compatible JSON descriptor, or
`--world-picker`. Browser URLs accept `?world=ID` and expose the picker only
with `?world-picker=1`. The picker preserves repository, revision, comparison
base and any file anchor. Installation binds a descriptor to a compiled
presentation adapter. An adapter owns its Bevy scene and declares the
interactions it implements. JSON supplies the presentation ID and palette.
A new adapter can be added without changing repository storage or services.

In Repo Reef, the lamp limits visibility during travel and in the map view.
At a station it lights the active reading surface evenly. Reading uses the
shared block timer. Completed lines retain a dim glow when the player
leaves. Paint marks use the shared review
anchors, including the file path, snapshot side and original line range.
The bathyscaphe saves its camera position per comparison and world. Private
survey buoys mark places in the water and can be retrieved on a later dive.

## Shared concepts

| Concept | Contract |
| --- | --- |
| World set | Stable ID, descriptor and registered presentation. The scene owns its geometry, camera pose, art and physical controls. |
| Repository comparison | Local checkout or GitHub repository with the revision, base and optional pull request. Opening it selects snapshots without checking out a commit. |
| File destination | Repository-relative path, snapshot side, presentation face and optional one-based range. Resolve against the current comparison and available content before moving. |
| Scene target | A directory, file, line, hunk, note or other addressable element projected from the comparison. Geometry resolves to the same identity as a list or link. |
| Interaction session | Owns current selection, input focus, route, history and device input. Menus, text editors, terminal, tablet and navigation prompts use the same focus arbitration. |
| Review state | Read credit, visits, comments, presets, preferences and private drafts. Persist it using existing repository, revision and content identities. Store a world-specific pose separately from the file and its reading position. |
| VibePad | A shared interactive screen with the current platform's tabs, data and actions. The world supplies the case, first-person hands, motion and form theme; the existing render target and input projection connect the screen to the scene. |

Code City and Repo Polis use the same directory key, including the root
directory. A building and a scroll representing the same path share file
identity. Persisted filesystem, GitHub, agent and review records do not
contain city or library geometry.

## Feature inventory

Availability follows the existing platform, repository, authorization and
provider conditions. A new view must expose the same available action in
that context before declaring it supported. Differences in animation,
geometry and movement style are presentation choices. The previews may
remain opt-in while a feature is still missing.

| Capability | Code City behavior to preserve | Repo Polis presentation |
| --- | --- | --- |
| Repository selection | Local folder, live worktree, frozen revision, recent checkout, GitHub repository or PR, revision and base selection. GitHub sign-in, private repo access and historical PR rules remain in effect. | Open the same comparison in the gallery. Keep the world and selection when changing repository or revision. |
| Frame and aim | First-person view, mouse capture and Esc, keyboard look, common input focus and pointer-based hover. The reticle and label identify the current target. | First-person eye camera and the same focus states. Render hands near the lower edge during tool use. Highlight the actual aimed cabinet or file and name it. |
| Travel | City flight, boost, altitude, street walking, wall anchoring, click/E approach and bookmarks or Back/Forward. | Walk in the aisles or take off as the owl. Land at the reading-room entrance before choosing a file. Click or press E on a scroll to approach and unroll it. Keep the selected file and any requested line. |
| Directory and project overview | District and project maps, directory selection, changed buildings and comment markers. | M fits every scroll in the current folder. M again fits all scrolls, arranged in reading rooms and districts for real top-level folders. Click a terrace for the folder map or select a scroll to land at its entrance. Escape cancels the map and flight. |
| File filtering and search | Changes-only filter, all files, search by filename and path, live save indicator, and a VibePad file list with changes, viewed files, and agent activity filters. | The same list and filters select visible file slots or open the matching scroll. Each folder occupies one room with alphabetically arranged cabinets. |
| Follow links and traces | File connections, import resolution, trace import, frame selection and route following. Confirmed paths are distinguishable from unresolved entries. | Use cross-references or aisle guidance. Keep the same resolved destination and trace order. |
| Source inspection | Syntax, exact line numbers, wrapping and horizontal movement, search and match navigation, fold/unfold, covered source previews and readable source. | Unroll a continuous scroll inside the room at a fixed scale per line. The remainder sits on a proportionally thicker roll. The wheel and keys feed paper under the overhead reading camera. |
| Comparison inspection | Source, diff and blame faces, including deleted lines, diff sides, hunks, before/after and empty-diff reason. Blame exposes author, commit and historical revision where available. | Present each face on the scroll or the shared reading surface. Preserve original source line numbers and actual comparison-side anchors. |
| Guided changed-file review | Changed roof tour, hunk buttons and paging, comparison route and return to the current diff. | Visit the corresponding changed scrolls and hunks with grounded navigation. |
| Reading credit | Frost, compact heat cue, time estimate, pause/resume, immediate mode, progress reset and content-bound persisted reveal. Visible or visiting is a separate fact. | The same reveal settings, timers, credit and reset apply to the scroll. Use a readable parchment presentation with visible blur or coverage and a small progress cue. |
| Saved place | Restore file, face and suitable camera position for a previously visited comparison. | Restore the corresponding file and reading location. Use the gallery pose when one is available. |
| File previews | P or the corresponding action opens text, Markdown, supported images and SVG, or video. Code preview has syntax coloring. Markdown links keep their repo/revision identity. | Reuse the same preview and media paths in the stone VibePad. |
| VibePad controls | Tab opens/puts away the physical screen. Click, wheel, scrollbars and keyboard focus select tabs and controls. Find, Help, Files, Settings, Git and other platform tabs share the existing state. | The same GPU screen and its projected input appear in a stone case held by first-person hands. Putting the case away keeps ongoing work or chat alive. |
| Help, actions and touch | Help/tour, right-click actions, reduced-input keyboard route and browser touch controls, with appropriate movement buttons. | The same actions and equivalent help are accessible. Touch moves and looks at walking height; tool and tablet controls retain their meaning. |
| Draft editing | Supported native and browser previews edit private drafts, use the existing syntax and keyboard editing, and act on the current file/range. Export or publication is explicit. | Use the same editor, preview changes, undo and publication paths. |
| Notes and paints | Select/edit presets, mark an eligible single line or range, open the mark, edit the comment, copy or publish. Markers, GitHub inline threads/mentions and viewed status show current state. | Put marks on their exact scroll or comparison rows with the existing palette, draft store and comment view. Preserve permission and anchor validation. |
| Navigation cues | Comment links, mentions, chat links and accepted MCP range proposals open exact paths, line ranges and sides. Follow-agent actions use explicit events. Guidance briefly highlights the range without awarding credit. | Walk to or open the corresponding scroll, honor the acceptance policy and reveal the same range. Keep the exact visible rows in the focus projection. |
| Git and terminal | Native Git history and exact comparison, persistent terminal with real checkout path and editor-safe focus. Web uses GitHub history without a local terminal. | Use the same tabs, state and terminal/focus rules on their supported platforms. |
| Native coding agents | Provider/model selection, messages, delivery and approvals, stop, sessions and persisted history. Agent activity and file links are inspectable. Codex voice and the existing provider policies follow their own availability. | Use the shared Agent/Chat views and routes. World visuals can show activity at the corresponding directory, scroll or line. |
| MCP and live worktree | Serving local notes, status, focus with visible/readable rows and blob IDs, proposals and sketches. Worktree updates keep identifiers and update files/navigation for the current checkout. | Publish the real scroll focus in the same MCP schema. Apply live updates without confusing one file or comparison side with another. |
| Cooperative rooms | Pinned comparison, roster, chat, invitations, share focus/lines/sketches and optional voice. Each member loads source with their own access. | Shared room controls and identity are available through the same tablet. Translate peer file destinations to the local scene. Keep private drafts and checkout data local. |
| Boards | Chat and README/document screens plus staged/shared Markdown and Mermaid sketches. | Place equivalent document and chat displays in the scene or on the shared tablet, rooted in the same comparison and room. |
| Settings and audio | Reading preferences, mute/volume, narration/radio, file-change chime, provider and input preferences with supported platform limits. | Use the same persisted settings. Theme changes can replace ambience and the character's visuals. |

View the platform details in [README](../README.md), review semantics in
[review spec](review-spec.md), and room and guidance acceptance in
[review parties](../openspec/changes/add-review-parties/specs/review-parties/spec.md)
and [agent reading guidance](../openspec/changes/add-agent-reading-guidance/specs/agent-reading-guidance/spec.md).
The inventory covers current product routes. A proposed feature in another
spec enters this contract when its supported route is available.

## Input and rendering requirements

- With a captured cursor, aim is the center of the player's view. With a free
  cursor, use the pointer ray from the viewport. The reticle, highlight, label,
  action and focus output resolve the same target in both states. Input owned
  by VibePad, a picker, text entry or a modal pauses world interaction.
- Use semantic destinations from search, notes, the tablet and agent tools.
  A world adapter resolves camera/placement independently, with common
  validation for snapshot, file, side and range. If a destination is unavailable
  in the current comparison, keep the view steady and explain the result.
- In the library, a cabinet represents one exact directory. Cabin dimensions
  increase with its file count up to the available space. Slots represent real
  paths. The corridor may reuse rendered bays while preserving these identities.
  Show directory names above visible cabinets and file names over nearby
  scrolls. Fade out labels outside the view and avoid overlap. The aimed label
  takes priority and the target caption shows the full path.
- Start library travel from the first-person camera. When the player approaches
  the coil, show stepping and handling from their own eyes. Unroll the
  whole file with consistent line height and keep a visibly larger roll for
  longer files. At rest the player's viewpoint and selected line stay stable.
  Scrolling moves the reading location continuously or by the requested
  step. A distant line jump may reposition the camera immediately.
  During unrolling the moving coil follows the edge of the paper along the
  floor. Reveal the printed rows with that edge, keeping the ink at the same
  physical scale throughout the animation.
- A skin supplies the case, tool/hands animation, lighting, material, splash
  art and form theme for VibePad. The library calls it Stone Pad. It uses
  stone panels, bronze accents and GFS Didot headings. Text over bronze uses
  dark ink. Use the common control implementations and keep code,
  notes, focus outlines, status colors and contrast legible. The screen
  dimensions, fitting, hit projection, accessible UI,
  open/close lifecycle and actions come from the shared tablet. The stone
  tablet must produce the same input outcomes as the city tablet.
- Keep common actions discoverable through input bindings and VibePad. In the
  library the reader's **J** may remain the line jump. The shared Notes action
  stays reachable at the focused line through the actions menu and the tablet.
- Keep Code City routes and defaults intact while adding library parity.
  The release must pass existing city scenarios and the corresponding library
  scenarios for each newly declared capability.

## Acceptance scenarios

| Given | When | Then |
| --- | --- | --- |
| The default launch and an opt-in launch open the same comparison | Open Code City, then Repo Polis with the world flag | Code City remains the default, the picker is opt-in, and the comparison and progress identities match. A custom descriptor chooses a registered presentation. |
| A small directory and a large directory both contain visible files | Aim at a cabinet, then at a file with mouse capture and with the free pointer | The larger cabinet is visibly taller, the correct target is highlighted and named, and E or a selection opens the intended file. A modal suppresses aim until closed. |
| A repository has multiple directories with scrolls | Press M twice, choose a cabinet from the corridor plan, then choose a scroll | The Stone Pad shows the cabinet and full-library maps in order. Selecting an entry opens the corresponding cabinet or file. Escape and an empty map leave the current location usable. |
| A file has 2,400 lines | Approach, unroll, wheel through it, then jump to line 1,750 and press End | The camera remains first-person, the animation shows the roll extending through the hall, the code and line numbers are legible, the highlight reaches line 1,750 and End reaches line 2,400. Recycled bays preserve scale. |
| A comparison has changed files and an active conversation or terminal | Open the stone VibePad, use a tab and its controls, then close and reopen it | The same tabs and actions work as in the city on the supported platform, and ongoing state survives. Mouse projection and keyboard focus reach the same control. |
| A changed or removed file has a saved note and a previous read visit | Select source, diff and blame, open the note, accept its destination and then reset progress | The correct side and anchor are shown, the note persists, and only the specified reading progress resets. |
| An MCP proposal or room cue identifies a line range | Accept the proposal or follow the authorized cue | The current world shows the exact readable rows without marking them read. Declining keeps the current view. |
| A snapshot is loading, a binary file has no source, or a target was deleted | Select its scroll, preview or link | Show the existing pending/error behavior and allow returning to the scene. Never present stale or unrelated source as current. |
| Public web, signed-in PR and native live checkout are available fixtures | Exercise picker, preview, note, history, agent and room actions in the supported contexts | World selection preserves existing authentication, permissions, live updates, note persistence and dispatch. Worktree files change only through the existing explicit edit/terminal paths. |
