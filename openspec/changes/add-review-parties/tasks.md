## 1. Room and access

- [ ] 1.1 Define the pinned comparison key, membership lease, invitation policy, and room message types.
- [ ] 1.2 Add browser creation, account-bound invitation, access verification, join, reconnect, removal, and close. Verify with mocked GitHub accounts that forwarding an invite or losing private PR access does not expose the room.
- [ ] 1.3 Verify that a fork PR can be opened at the pinned comparison and remains pinned when GitHub reports a newer head.

## 2. Shared review

- [ ] 2.1 Add the party roster and invite controls to the city UI. Indicate whether a room is connected, reconnecting, or closed.
- [ ] 2.2 Stream coalesced poses and render peer avatars. Verify a pose burst cannot delay a join, removal, focus update, or line cue behind the queued poses.
- [ ] 2.3 Add opt-in live reading focus to the building and roster. Verify updates on file and side changes, clearing during flight and idle, and jumps with or without a line anchor.
- [ ] 2.4 Suspend auto-publish for the room and verify two reviewers can make private drafts without producing GitHub comments.
- [ ] 2.5 Send an explicit line cue and route the recipient to it. Verify that it outlasts live focus, works for both sides of a diff, and leaves the camera free until selected.

## 3. Voice

- [ ] 3.1 Add authorized WebRTC signaling, relay fallback, microphone permission, push-to-talk, speaker indication, and local mute.
- [ ] 3.2 Test audio across two independent networks and exercise a four-person room. Test denied microphone permission, focus loss while transmitting, removal, and disconnect with voice active.

## 4. Mac and rollout

- [ ] 4.1 Reuse Mac GitHub sign-in for room credentials and bring the roster, movement, live focus, and line cues to Mac.
- [ ] 4.2 Run Mac and web clients in the same room. Verify media permission, direct and relayed audio, and the same removal and publishing behavior on both platforms.
- [ ] 4.3 Document supported clients and controls. Exercise the feature switch and room shutdown before leaving the pilot.
