## Why

Agent navigation reaches a file while unread source remains covered. Interrupted reading also loses its unfinished timer. Reviewers need to see the code an agent is working on and resume reading after looking away.

## What Changes

- Add an opt-in Follow agent control. Explicit file activity and accepted MCP navigation show the source block at the target line immediately.
- Count only focused reading time. Keep unfinished block timers during the current city session, including visits to other files. Clear timers on progress reset; changed content never inherits an old timer.
- Keep temporary visibility separate from committed reading progress. A block becomes read only after its accumulated reading delay.
- Replace opaque unread code strips with blurred syntax in close tiles and distant source previews. Use a compact heat tool and the existing cursor timer.
- Open and briefly highlight the complete source range after accepted MCP navigation or a comment's Fly action, in both browser and native clients.
- Add private local MCP range notes with content identity and idempotent creation. Refresh them in the running native city without requiring restart.
- Bind saved reading ranges to content; carry only unambiguous unchanged read lines into updated source.
- Keep hot patching and multiplayer implementation outside this change. Shared room voice and agent audio routing extend the existing review-parties proposal.

## Capabilities

- `agent-reading-guidance` covers navigation, temporary visibility and accumulated reading time.

## Impact

The native agent controls and city navigation select a source location. Shared reading logic owns unfinished timers for source, diff and blame. Existing progress storage continues to persist completed reveals; unfinished timers do not survive application exit in this first version.
