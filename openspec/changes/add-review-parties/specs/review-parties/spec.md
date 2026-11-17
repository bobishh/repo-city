## ADDED Requirements

### Requirement: Invite a GitHub account to a review party

Repo City SHALL let a signed-in reviewer start a private party for an open or historical GitHub pull request they can read. The party SHALL admit up to four accounts including the host. Each invitation SHALL be time limited, revocable by the host, and bound to one verified GitHub account ID.

#### Scenario: Invited reviewer joins

- **WHEN** the invited account opens a valid invitation and can read the pinned comparison with its own GitHub authorization
- **THEN** Repo City joins the party and adds that account to the roster

#### Scenario: Another account opens a forwarded link

- **WHEN** a different account opens an invitation issued to another GitHub account
- **THEN** Repo City denies entry without showing the party's repository, participants, or presence

#### Scenario: Invitation expires or the party is full

- **WHEN** the invitee opens an expired or revoked invitation, or attempts to join a room at capacity
- **THEN** Repo City shows the reason and keeps the invitee outside the room

### Requirement: Keep participants on a pinned comparison

Members SHALL enter a room only after loading its exact PR base and head and selecting a compatible layout. Each member SHALL load source through their own GitHub access. The room SHALL verify continued access before renewing membership.

#### Scenario: PR head changes during review

- **WHEN** a new commit arrives after the room starts
- **THEN** the room keeps the pinned comparison and shared locations continue to resolve against it

#### Scenario: Invitee cannot open a private comparison

- **WHEN** GitHub denies the invited account access or that account cannot load the pinned snapshot
- **THEN** it receives an access or snapshot error before any room metadata, source, or voice is shared

#### Scenario: A member loses GitHub read access

- **WHEN** a renewal cannot confirm the member's continued access
- **THEN** their membership, presence stream, and access to party voice end by the current lease's expiry

### Requirement: Share live presence and review focus

Repo City SHALL show each member's name and live position in the city. Each member SHALL be able to opt in to showing their current reading focus to their party. While they are viewing code, focus SHALL identify the file and side and include an anchor when known. Repo City SHALL show this focus at the building and in the party roster. Each member SHALL be able to explicitly share a fixed file, side, and line range. Either indicator SHALL offer a jump for a recipient while leaving the recipient's camera under their control until selected.

#### Scenario: See and join a teammate's current reading view

- **WHEN** a member with live focus enabled opens code on a building
- **THEN** teammates see that member's indicator on the building and the current file and side in the roster
- **AND** selecting the indicator flies the teammate to that side and uses the current line anchor when available

#### Scenario: Switch between code views

- **WHEN** a member with live focus enabled switches to another file or side
- **THEN** their badge and roster entry reflect the new focus and no badge remains at their former view

#### Scenario: Clear live reading focus

- **WHEN** a member leaves the active code view to fly, goes idle, turns off live focus, or disconnects
- **THEN** their reading indicator clears from the building and roster
- **AND** any line range they explicitly shared during the party remains available

#### Scenario: Keep reading focus private

- **WHEN** a member views code with live focus disabled
- **THEN** their presence stays visible to the party without broadcasting their current file or side

#### Scenario: Join a teammate at a diff

- **WHEN** a member shares a line range and another selects that cue
- **THEN** Repo City flies the recipient to the same file and positions the camera at that range on the specified side

#### Scenario: Keep flying after a teammate changes focus or shares lines

- **WHEN** another member changes their live focus or shares a range and the recipient keeps flying
- **THEN** the focus indicator or cue updates without moving the recipient's camera

#### Scenario: Remove a member from the party

- **WHEN** the host removes a member
- **THEN** Repo City ends that member's room and media connections, removes their avatar from other clients, and rejects reconnect attempts without a new invitation

### Requirement: Speak on demand in the party

Where voice is available, Repo City SHALL request microphone access only when a member explicitly enables voice. The microphone SHALL send audio only while that member holds push-to-talk. Members SHALL be able to see who is speaking and mute another member locally. Repo City SHALL stop sending audio on release, focus loss, leave, removal, or explicit disable. The service SHALL keep voice ephemeral.

#### Scenario: Speak and release the control

- **WHEN** an enabled member holds push-to-talk and then releases it
- **THEN** unmuted listeners hear audio and see the speaker indicator while the control is held, and transmission stops on release

#### Scenario: Microphone permission is denied

- **WHEN** a member denies microphone permission
- **THEN** Repo City displays that voice is unavailable and the member can continue using presence and shared locations

#### Scenario: Leave or lose focus while speaking

- **WHEN** the transmitting client leaves the room or loses window focus
- **THEN** audio transmission stops and any later transmission requires a fresh push-to-talk press

### Requirement: Keep draft notes and publication under the author's control

Party traffic SHALL contain only membership, presence, opt-in reading focus, explicitly shared locations, and voice transport. Focus indicators and line cues SHALL carry location metadata without source text or diffs. Draft bodies and reading progress SHALL stay with the reviewer. Entering a party SHALL suspend auto-publication for the session. Each GitHub publication SHALL require the author to choose the existing publish action.

#### Scenario: Paint code while talking

- **WHEN** a party member creates a review draft and shares that location
- **THEN** other members receive only the location, and GitHub receives a comment only if its author chooses to publish it

### Requirement: Reconnect or end a party

Repo City SHALL allow a previously admitted account to resume while its membership is still valid. Closing the party or the end of the host's reconnect window SHALL terminate room access and remove temporary room cues.

#### Scenario: Brief interruption

- **WHEN** a member reconnects before the room ends and their access is still valid
- **THEN** they regain the current roster and presence without duplicate membership or replay of previous audio

#### Scenario: Host closes the room

- **WHEN** the host closes the room
- **THEN** members return to the pinned comparison in solo mode, their drafts stay available, and outstanding invites no longer admit anyone
