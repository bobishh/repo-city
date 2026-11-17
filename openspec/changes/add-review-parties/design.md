## Context

Repo City already signs in with its GitHub App on web and Mac. Each client loads a GitHub PR comparison and stores its own review drafts. The web Worker keeps GitHub credentials in an encrypted session cookie and calls GitHub on behalf of the signed-in reviewer.

Paket's session at source revision `7027ef350c4428805b2bd8f06f8d24fad57d7688` uses Iroh and MetaMesh gossip to share delivery events and actor poses. Its links encode a street origin, peer address, and room. Its native worker coalesces recent poses and polls independently of the game loop. Its two-peer tests cover receiving a pose and receiving a delivery after a pose burst. Repo City can apply the same coalescing and test pattern to flying and review cues.

## Goals / Non-Goals

The first supported party is an invite-only review of one GitHub pull request, with two to four people including the host. Members can see who's present, share their live reading focus, point to a specific line range, and speak on demand.

Worktree sharing, persistent party history, shared draft editing, voice recording, and user discovery outside the invite picker are outside this proposal.

## Decisions

### GitHub identities and access

Resolve the authenticated numeric account ID through GitHub's `GET /user` and use the login and avatar for display. Resolve each invited login to an ID when issuing the invite. Bind the invite to that ID and the room. A forwarded link cannot admit a different account. Use the browser's existing authenticated session. For Mac, exchange the existing GitHub credential at the service for a short-lived room credential bound to the verified account and room. Peers, invite links, and signaling payloads never receive GitHub credentials. [GitHub authenticated user API](https://docs.github.com/en/rest/users/users#get-the-authenticated-user).

Check each member's access with their own GitHub credential, using the same PR and snapshot the client needs to load. Recheck on rejoin and renew access on a short lease during the session. End membership and close media on expiry, access loss, or host removal. The room credential grants party access only. [GitHub pull request API](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request).

### One pinned comparison

Store one comparison key with the base repository ID, PR number, merge base SHA, and head SHA. Each member loads those exact GitHub snapshots with their own authorization. Use a shared layout version for coordinate compatibility. For jumps, the client resolves the file path and side locally and uses the line anchor when available. A newer PR head is available for a new room, while the open room keeps its original comparison. If a pinned snapshot becomes unavailable, show an error and keep the user outside the shared view.

### Room ownership and updates

Use one hosted authority per room for invites, membership, signaling, and presence. Start from the existing authenticated web entry point and validate the room implementation behind it. The host can admit invited accounts, revoke unused invites, remove members, and close the room. A temporary disconnect leaves a short reconnect window, after which the host's absence ends the room. Members reconnect with the same account and a valid membership.

Use an authenticated socket for presence and review cues. Keep the latest pose per participant with a sequence number, interpolate remote movement on the client, and discard stale updates. Use bounded queues and give joins, removals, focus changes, and shared line ranges priority over pose updates. Paket's render/transport boundary can inform the adapter and tests. Record provenance if implementation copies any code.

Using Paket's peer gossip directly would also require a separate service for identity, read permissions, and revocation. A small room already has a place to enforce those rules. Start with the room socket and evaluate Iroh if direct native transport solves a measured connectivity or latency problem. Evaluate MetaMesh if multiple product features later need shared discovery or distributed state.

### Push-to-talk

Use WebRTC audio with room-authorized signaling and STUN/TURN for the first four-person release. The browser already provides media capture and playback APIs. The Mac implementation must pass an interoperability and microphone permission test before voice becomes available there. Enable the microphone only after the member chooses voice and grants permission. Send audio while push-to-talk is held, and stop the local track on focus loss, leave, removal, or explicit disable. Show who is speaking and let each member mute individual peers. The existing room authority tears down authorized media connections on removal. [W3C WebRTC specification](https://www.w3.org/TR/webrtc/).

A relay can pass encrypted audio in transit. Neither the room nor its media relay stores audio, SDP, ICE candidates, or the contents of review cues after use. If parties outgrow four-person peer audio or direct peers' network visibility proves unacceptable, measure a relay-only setup or a media router before raising the limit.

### Live reading focus and line cues

Offer a live focus toggle to each member. When enabled, use the active code view to update that member's focus with a file path, side, and line anchor when known. Publish on change and clear on leaving the code view, becoming idle, turning the toggle off, or disconnecting. The room holds at most one current focus per member and includes current focus in roster updates. The world-set adapter shows an indicator at the relevant building with the reader's identity. The roster shows that same member's file and side.

An explicit line cue carries a fixed range and the sender's identity. It stays available for the party when the sender switches code views or clears live focus. Selecting a live indicator or a line cue triggers a local jump for that recipient. Text, diffs, GitHub credentials, paints, reading progress, and draft bodies are absent from room messages.

The existing review flow saves, exports, and publishes under the active reviewer's account. Suspend any enabled auto-publish setting on party entry.

## Risks / Trade-offs

- GitHub access can change while people are in a room. Short leases bound the gap between losing GitHub access and losing party access. A failed renewal ends the current lease.
- Versions can disagree on where a building is. Negotiate a supported layout version before rendering other members and use semantic anchors for shared jumps.
- Media routing can fail on restrictive networks. Test relay connectivity on separate networks and keep shared navigation usable if voice fails.
- A burst of poses can hide joins or slow navigation. Bound work per frame, coalesce movement, and test reliable room events during bursts.

## Migration Plan

1. Gate creation and join behind a disabled-by-default release setting. Pilot browser rooms with two and four authenticated users, including a private fork PR with distinct permissions.
2. Enable browser voice for pilot rooms after direct and relayed tests pass. Keep navigation available when voice is unavailable.
3. Add Mac parity for invites and movement, then enable voice when Mac and browser interoperate. Rollback disables new rooms and closes existing rooms after a grace period. No draft or review data migration is needed.

## Open Questions

- Choose the room deployment and media relay after comparing existing hosting, operating cost, and results from two independent networks.
- Measure the Mac audio adapter for device selection, capture permissions, and cross-platform playback before choosing its implementation.
