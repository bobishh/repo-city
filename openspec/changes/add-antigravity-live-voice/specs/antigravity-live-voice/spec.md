## ADDED Requirements

### Requirement: Start built-in Antigravity voice

Repo City SHALL provide an explicit desktop voice control for Antigravity. It SHALL open the interactive Antigravity CLI using its existing account sign-in and let the CLI manage microphone and speaker audio.

#### Scenario: Open voice with an Antigravity account

- **GIVEN** Antigravity is selected and the user has signed in to its CLI
- **WHEN** the user starts voice in VibePad or presses F5
- **THEN** VibePad opens the Antigravity terminal and starts its built-in voice mode using that account

### Requirement: Preserve the conversation during handoff

Repo City SHALL release the ACP session after the current turn completes. It SHALL resume its conversation ID in the terminal and confirm the identity before sending the voice shortcut. The terminal SHALL use the workspace sandbox and display Antigravity's permission prompts.

#### Scenario: ACP is working when voice is requested

- **GIVEN** an Antigravity turn is running through ACP
- **WHEN** the user requests voice
- **THEN** the active turn completes before the terminal takes over its conversation

#### Scenario: The terminal resumes a different conversation

- **GIVEN** a voice session is requested for an existing ACP conversation
- **WHEN** the terminal reports a different conversation ID
- **THEN** Repo City does not send its voice start shortcut

### Requirement: Stop voice while keeping terminal work

Repo City SHALL use the CLI voice stop shortcut without closing the terminal. It SHALL reconnect ACP to the saved conversation after the interactive terminal exits.

#### Scenario: The user hangs up during CLI work

- **GIVEN** Antigravity is running a task in the voice terminal
- **WHEN** the user stops voice through Repo City
- **THEN** Antigravity ends voice and continues the task in the terminal
