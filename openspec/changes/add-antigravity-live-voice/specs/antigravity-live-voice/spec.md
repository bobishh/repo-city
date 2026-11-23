## ADDED Requirements

### Requirement: Use native Gemini audio in Antigravity voice mode

Repo City SHALL provide an explicit voice control with Antigravity selected. The client SHALL send microphone audio to Gemini Live and play the model's audio output. It SHALL clear stale playback when Gemini reports an interruption. The voice connection SHALL require separate Gemini API credentials and surface failures without stopping the ACP text session.

#### Scenario: Start and interrupt a spoken reply

- **GIVEN** Antigravity is selected and Gemini Live credentials are configured
- **WHEN** the user starts voice and speaks, then interrupts a spoken reply
- **THEN** Repo City plays Gemini's audio and discards the interrupted reply's queued audio

#### Scenario: Credentials are missing

- **GIVEN** no Gemini API key is configured
- **WHEN** the user starts voice
- **THEN** Repo City displays the configuration error and keeps chat available

### Requirement: Keep spoken work in the active ACP session

The voice connection SHALL submit, steer, check and stop work through the Antigravity adapter associated with the active checkout. Repository permission decisions SHALL use the active Repo City permission mode. Ending voice SHALL release audio resources without canceling the ACP turn.

#### Scenario: Ask a question while Antigravity works

- **GIVEN** an active Antigravity conversation and an open voice connection
- **WHEN** the user delegates a task and asks for progress before it finishes
- **THEN** the task uses the same ACP conversation and the voice connection can report its progress and result

#### Scenario: Return to typing during a task

- **GIVEN** a task is still running in the Antigravity conversation
- **WHEN** the user stops voice
- **THEN** microphone capture stops and the task continues in Chat
