## Context

Repo City already signs in with its GitHub App on web and Mac. Each client loads a GitHub repository comparison and stores its own review drafts. Phoenix keeps web GitHub credentials in an encrypted session cookie and calls GitHub on behalf of the signed-in reviewer.

Paket's session at source revision `7027ef350c4428805b2bd8f06f8d24fad57d7688` uses Iroh and MetaMesh gossip to share delivery events and actor poses. Its links encode a street origin, peer address, and room. Its native worker coalesces recent poses and polls independently of the game loop. Its two-peer tests cover receiving a pose and receiving a delivery after a pose burst. Repo City can apply the same coalescing and test pattern to flying and review cues.

## Goals / Non-Goals

The first supported party is an invite-only review of one pinned GitHub repository comparison, with or without a pull request. An account-bound invitation link opens the same repository, revision and comparison base after access verification. Members can see who's present, share their live reading focus, point to a specific line range, and speak on demand. Supported capacity follows measured network, client, and service budgets; it is not a fixed number of GitHub accounts or a claim of unlimited scale.

Worktree sharing, persistent party history, shared draft editing, voice recording, and user discovery outside the invite picker are outside this proposal.

## Decisions

### GitHub identities and access

Resolve the authenticated numeric account ID through GitHub's `GET /user` and use the login and avatar for display. Resolve each invited login to an ID when issuing the invite. Bind the invite to that ID and the room. A forwarded link cannot admit a different account. Use the browser's existing authenticated session. For Mac, exchange the existing GitHub credential at the service for a short-lived room credential bound to the verified account and room. Peers, invite links, and signaling payloads never receive GitHub credentials. [GitHub authenticated user API](https://docs.github.com/en/rest/users/users#get-the-authenticated-user).

Check each member's access with their own GitHub credential, using the same PR and snapshot the client needs to load. Recheck on rejoin and renew access on a short lease during the session. End membership and close media on expiry, access loss, or host removal. The room credential grants party access only. [GitHub pull request API](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request).

### One pinned comparison

Store one comparison key with the base repository owner/name, optional PR number, exact base SHA, exact head SHA, and layout version. Fork pull requests also retain the head repository identity. Each member loads those exact GitHub snapshots with their own authorization. Use a shared layout version for coordinate compatibility. For jumps, the client resolves the file path and side locally and uses the line anchor when available. A newer PR head is available for a new room, while the open room keeps its original comparison. If a pinned snapshot becomes unavailable, show an error and keep the user outside the shared view.

### Room ownership and updates

Use one hosted authority per room for invites, membership, signaling, and presence. Start from the existing authenticated web entry point and validate the room implementation behind it. The host can admit invited accounts, revoke unused invites, remove members, and close the room. A temporary disconnect leaves a short reconnect window, after which the host's absence ends the room. Members reconnect with the same account and a valid membership.

Use an authenticated socket for browser presence and review cues. Desktop uses equivalent room-scoped HTTP event submission and cursor polling; these endpoints enforce the same membership lease. Keep the latest pose per participant with a sequence number, interpolate remote movement on the client, and discard stale updates. Use bounded queues and give joins, removals, focus changes, and shared line ranges priority over pose updates. Paket's render/transport boundary can inform the adapter and tests. Record provenance if implementation copies any code.

Using Paket's peer gossip directly would also require a separate service for identity, read permissions, and revocation. Phoenix runs the room authority on Hetzner with one supervised process per room. It serializes membership changes and enforces access for both transports. Active rooms live in memory and end on a backend restart. Each room has an independent ID and a comparison hash derived from repository ID, base, head, and layout version. The media topology is authorized peer WebRTC mesh with optional STUN/TURN.

### Push-to-talk

Use WebRTC audio with room-authorized signaling and STUN/TURN. The browser already provides media capture and playback APIs. The Mac implementation must pass an interoperability and microphone permission test before voice becomes available there. Enable the microphone only after the member chooses voice and grants permission. Send audio while push-to-talk is held, and stop the local track on focus loss, leave, removal, or explicit disable. Show who is speaking and let each member mute individual peers. The existing room authority tears down authorized media connections on removal. [W3C WebRTC specification](https://www.w3.org/TR/webrtc/).

Neither the room nor its media relay stores audio, SDP, ICE candidates, or the contents of review cues after use. Evaluate direct peer mesh, TURN-relayed mesh, and an SFU against the capacity model below. TURN addresses reachability; a relay-only ICE policy can avoid sharing direct candidates. It does not remove mesh stream duplication. An SFU forwards media through a server and has a different bandwidth and encryption trust boundary, which must be documented before enabling that topology. [TURN specification](https://www.rfc-editor.org/rfc/rfc8656.html).

### Party conversation and local coding agents

Keep party voice and agent voice as independent destinations. Capture the local microphone once and fan out its clean samples to the enabled destinations. A member who enables party conversation input for their connected agent supplies local microphone audio and authorized remote participants' audio to that agent. Each member controls their own agent destination; two members can enable it and both agents then receive both participants. Expose this routing to everyone in the room before forwarding remote voices. Agent disconnection stops that destination immediately and does not stop human conversation.

Feed decoded remote participant streams before speaker playback; never recapture the loudspeaker mix. Exclude agent-generated speech from all agent inputs and party uplinks to prevent agent feedback loops and duplicate forwarding. Preserve participant identity through the routing boundary. Agent output is local-only in this first version. Validate echo cancellation, simultaneous speech and voice gating before claiming full-duplex interoperability; the current solo voice path pauses capture during agent playback.

### Capacity and connection health

Keep room admission, voice availability, and simultaneous transmission separate. A reviewer can remain in presence and shared navigation when their voice path is unavailable. Record the actual topology and negotiated media parameters before estimating capacity.

For a planning model, let `N` be admitted members, `K` concurrent voice senders, `b` the encoded bitrate of one active audio stream in bits/s, `r` the emitted pose updates per member per second after coalescing, and `p` the encoded bytes per pose update. Let `s_i` be 1 while member `i` transmits and 0 otherwise. The following are payload rates, assuming equal stream bitrates and all-to-all delivery with voice enabled for every member. For presence-only members, use the actual voice-endpoint count instead of `N` in voice calculations; presence still uses the full roster:

| Path | Payload rate |
| --- | --- |
| Mesh: one member's voice upload | `s_i (N - 1) b` |
| Mesh: one member's voice download | `(K - s_i) b` |
| Mesh: aggregate voice upload across members | `K (N - 1) b` |
| Mesh: aggregate voice upload when everyone talks | `N (N - 1) b` |
| Hosted presence: one member's upload | `8 r p` |
| Hosted presence: one member's download | `8 (N - 1) r p` |
| Hosted presence: room service ingress / egress | `8 N r p` / `8 N (N - 1) r p` |
| Candidate SFU: one member's voice upload / download | `s_i b` / `(K - s_i) b` |
| Candidate SFU: media service ingress / egress | `K b` / `K (N - 1) b` |

The SFU model assumes one upstream stream per talker and forwarding to every other member, without mixing. A TURN-relayed mesh retains the mesh's endpoint upload requirement and adds relay traffic. Actual relay hops, packetization, RTP/SRTP, UDP/IP or TCP/TLS, control traffic, silence behavior, and recovery traffic must be measured without double counting. Idle connections still have transport cost when `K = 0`.

For an explicitly illustrative calculation, choose `N = 10`, `K = 2`, and `b = 24,000 bits/s`. This is an assumed Opus payload bitrate within its supported range, not a protocol default or a measured product setting. Mesh upload is 216 kbit/s per talker; download is 24 kbit/s for a talker and 48 kbit/s for a silent listener. Aggregate voice upload is 432 kbit/s; if all ten talk it is 2.16 Mbit/s. An SFU would instead receive 48 kbit/s and forward 432 kbit/s. These figures exclude transport overhead. [Opus over RTP](https://www.rfc-editor.org/rfc/rfc7587.html#section-3.1).

If the same example assumes `r = 10 updates/s` and `p = 120 bytes`, each member sends 9.6 kbit/s of poses and receives 86.4 kbit/s. The room service receives 96 kbit/s and sends 864 kbit/s. These assumed values describe a reproducible calculation, not chosen defaults. Pose fanout can cost more than speech and must adapt too.

For each member, reserve configurable headroom and account for presence, control traffic, other network use, and measured wire overhead before allocating voice upload `U_i` and download `D_i`. In uniform-bitrate mesh, a member who may transmit needs `(N - 1) b <= U_i`, while receiving needs `(K - s_i) b <= D_i`. For example, the upload-only bound is `N <= 1 + floor(U_i / b)`; it is not sufficient on its own. Check every affected endpoint, service ingress/egress, and client CPU/frame budgets. Full mesh has `N (N - 1) / 2` peer relationships even when few members talk. Measure actual encoder reuse, active decoders, mixing, cryptography, and avatar/presence work rather than assuming an implementation's CPU cost from member count.

Sample browser `RTCPeerConnection.getStats()` over configured windows: derive traffic rates from byte-counter deltas; correlate audio loss, jitter, concealment, RTT, and the selected candidate path. `availableOutgoingBitrate` is a candidate-pair estimate, not the entire uplink; do not add peer estimates as independent capacities. RTP byte counters omit transport headers. Do not use video-only `qualityLimitationReason` as an audio health signal. Treat absent fields as unknown and use native equivalents on Mac. [W3C statistics definitions](https://www.w3.org/TR/webrtc-stats/).

Coalesce pose updates and reduce their rate before allowing queues to grow; keep membership, focus, and line cues prioritized. Adapt negotiated audio quality where supported; `RTCRtpSender.setParameters()` bitrate limits do not guarantee a viable codec or network path. If acceptable voice cannot be sustained, show voice as unavailable while retaining presence when its own budget permits. Evaluate a relay or SFU for the measured failure it solves. Admission or transmission restrictions must show the affected resource and recovery action. Keep measurement windows, headroom, quality targets, and any operational ceilings configurable and backed by reproducible load tests; do not invent universal thresholds. [WebRTC sender parameters](https://www.w3.org/TR/webrtc/#dom-rtcrtpsender-setparameters).

### Live reading focus and line cues

Offer a live focus toggle to each member. When enabled, use the active code view to update that member's focus with a file path, side, and line anchor when known. Publish on change and clear on leaving the code view, becoming idle, turning the toggle off, or disconnecting. The room holds at most one current focus per member and includes current focus in roster updates. The world-set adapter shows an indicator at the relevant building with the reader's identity. The roster shows that same member's file and side.

An explicit line cue carries a fixed range and the sender's identity. It stays available for the party when the sender switches code views or clears live focus. Selecting a live indicator or a line cue triggers a local jump for that recipient. Text, diffs, GitHub credentials, paints, reading progress, and draft bodies are absent from room messages.

The existing review flow saves, exports, and publishes under the active reviewer's account. Suspend any enabled auto-publish setting on party entry.

## Risks / Trade-offs

- GitHub access can change while people are in a room. Short leases bound the gap between losing GitHub access and losing party access. A failed renewal ends the current lease.
- Versions can disagree on where a building is. Negotiate a supported layout version before rendering other members and use semantic anchors for shared jumps.
- Media routing can fail on restrictive networks. Test relay connectivity on separate networks and keep shared navigation usable if voice fails.
- A burst of poses can hide joins or slow navigation. Bound work per frame, coalesce movement, and test reliable room events during bursts.
- A larger roster can exhaust an endpoint or service even with few talkers. Measure both media and presence fanout; do not interpret low voice traffic as spare capacity everywhere.

## Migration Plan

1. Gate creation and join behind a disabled-by-default release setting. Use two- and four-user browser rooms as baseline test fixtures, not membership limits. Increase room size and concurrent talkers under controlled network and CPU conditions, including a private fork PR with distinct permissions.
2. Enable browser voice for pilot rooms after direct and relayed tests pass. Keep navigation available when voice is unavailable.
3. Add Mac parity for invites and movement, then enable voice when Mac and browser interoperate. Rollback disables new rooms and closes existing rooms after a grace period. No draft or review data migration is needed.

## Open Questions

- Choose the room deployment and media topology after comparing existing hosting, operating cost, and direct/relayed results from two independent networks. Establish configurable quality targets and resource budgets from those measurements.
- Measure the Mac audio adapter for device selection, capture permissions, and cross-platform playback before choosing its implementation.
