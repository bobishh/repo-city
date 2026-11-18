## Why

A Repo City party lets reviewers fly through the same pull request and see which building and side a teammate is reading. Reviewers can fly to a teammate's current view and discuss the code over voice.

## What Changes

- Let a signed-in reviewer start a party from a GitHub pull request and invite people by GitHub account.
- Put authorized reviewers in the same pinned comparison. When a reviewer enables live focus, show the file and side they are reading on the building and in the party roster. Selecting the indicator flies to that view.
- Let reviewers share a specific line range as a separate cue for the group to revisit.
- Add opt-in push-to-talk with a speaking indicator, individual mute, and host controls to remove a member or close the room.
- Keep party cues ephemeral. Each reviewer continues to control their own GitHub comments and local drafts.
- Ship presence and navigation first, then voice and Mac interoperability. Derive supported room capacity from measured network, client, and service budgets instead of a fixed account count.
- Adapt pose traffic and voice quality to connection health. Explain resource limits and keep shared navigation available when voice cannot be sustained.

## Capabilities

### New Capabilities

- `review-parties` covers invitations, shared review presence and navigation, and voice.

### Modified Capabilities

## Impact

The change extends GitHub sign-in, PR navigation, and the city UI. It adds a room service for membership and presence and a media path for voice. GitHub remains the source of repository access and published reviews.
