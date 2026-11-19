# webrtc-ice 0.14.0

Source: the webrtc-ice 0.14.0 crates.io release, from webrtc-rs/webrtc revision
`a1f8f1919235d8452835852e018efd654f2f8366`, directory `ice`.
MIT OR Apache-2.0; see the bundled licenses.

Local changes are confined to `src/agent/agent_internal.rs` and
`src/agent/agent_selector.rs`. The ICE keepalive clock runs independently of
RTP traffic. On the selected pair, the controlling peer refreshes consent
using the same authenticated nomination check it used to select that pair.
This matches browser behavior for Codex Realtime. Controlled peers use an
ordinary authenticated check. Response validation, the liveness timeout and
candidate selection follow the upstream implementation.

Only the native Codex adapter uses this crate in Repo City. The browser uses
its own WebRTC implementation. See Mozilla bug 2037009 for the interoperability
case and pion/ice's selected-pair consent checks for the periodic scheduling
approach.
