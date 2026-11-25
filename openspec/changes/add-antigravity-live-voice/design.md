## Voice session ownership

The voice control waits for the active ACP turn to finish, closes the ACP session and opens the same conversation in the existing VibePad pseudoterminal. The CLI receives the model confirmed by ACP when available. Repo City waits for the interactive CLI to report the expected conversation and show an empty prompt before submitting `/voice live`. Antigravity handles microphone audio, playback, transcripts, workspace trust and tool approval in the terminal. The regular terminal shell has its own process.

The voice control sends Antigravity's hang-up shortcut while the terminal stays available. Once the interactive CLI exits, Repo City can reconnect ACP to the same conversation. Repo City stores its local Chat transcript separately from the CLI transcript.
