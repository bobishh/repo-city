## Connection

The Antigravity engine wraps the existing ACP adapter with an optional Gemini Live connection. The native audio pipeline captures mono PCM and plays the model's audio. The Gemini adapter converts between the native sample rate and the API's input and output rates, and clears queued output on a reported interruption.

## Conversation

The Companion voice connection sends work requests through the existing ACP adapter. A voice request refers to the selected checkout and ACP conversation. The model can ask for progress or explicitly stop work while the ACP turn continues. The voice connection returns bounded text from the Antigravity conversation. Repository permission requests remain under the existing Repo City controls.

## Lifecycle and credentials

Starting voice requires an explicit user action. The application uses a configured Gemini API key and exposes connection errors in the existing voice status. Stopping voice closes the audio connection while Antigravity work continues. A closed or failed voice connection does not close the Antigravity text session.
