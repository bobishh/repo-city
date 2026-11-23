## Why

Repo City users can speak with Codex while reviewing code. Antigravity supports native speech in its own clients, but its ACP server does not accept audio. Repo City needs a voice connection that keeps the selected Antigravity conversation and its permissions.

## What Changes

- Add Gemini Live voice input and native model audio output for Antigravity on desktop.
- Let the voice session start, steer, check and stop work in the active Antigravity conversation.
- Use the existing voice controls and audio devices. Keep microphone activation opt-in and preserve permission prompts in Repo City.

## Capabilities

- `antigravity-live-voice` covers voice activation, audio playback, and delegation to the selected ACP conversation.

## Impact

The desktop engine uses the public Gemini Live API with separate credentials. Antigravity still owns repository operations over ACP. Browser builds and the Codex voice transport retain their existing behavior.
