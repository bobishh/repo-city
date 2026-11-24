## Why

Antigravity users already sign in through the CLI. Repo City should start Antigravity's built-in voice mode with that account.

## What changes

- Add a desktop control that opens the current Antigravity conversation in VibePad's terminal and starts native voice.
- Leave voice audio, transcripts and tool interaction to Antigravity while its terminal owns the conversation.
- Return the conversation to ACP when the terminal exits.
