//! Codex adapter: maps app-server messages into the shared agent session contract.
use super::codex_transport as transport;
use super::{CODEX, Command, Engine, Event, Projection, Provider, Start};
use std::path::PathBuf;

pub(super) struct Adapter {
    root: PathBuf,
    client: transport::Client,
}
impl Adapter {
    pub fn start(start: Start) -> Self {
        Self {
            client: transport::Client::start(
                start.root.clone(),
                start.conversation,
                start.speaking,
                start.diagnostics,
            ),
            root: start.root,
        }
    }
}
impl Engine for Adapter {
    fn provider(&self) -> Provider {
        CODEX
    }
    fn send(&self, command: Command) -> Result<(), String> {
        use transport::Command as Wire;
        self.client.send(match command {
            Command::Send { prompt, context } => Wire::Send { prompt, context },
            Command::Steer { prompt, context } => Wire::Steer { prompt, context },
            Command::Voice { id, context } => Wire::Voice { id, context },
            Command::StopVoice => Wire::StopVoice,
            Command::Interrupt => Wire::Interrupt,
            Command::Approval { token, accept } => Wire::Approval {
                id: serde_json::from_str(&token).map_err(|_| "Invalid agent approval token.")?,
                accept,
            },
        })
    }
    fn poll(&self, state: &mut Projection) -> Vec<Event> {
        let mut events = Vec::new();
        for event in self.client.poll() {
            use transport::Event as Wire;
            match event {
                Wire::Connected(id) => {
                    state.dialogue.thread = Some(id.clone());
                    events.push(Event::Connected(id));
                }
                Wire::Replaced { thread, .. } => events.push(Event::Replaced {
                    conversation: thread,
                }),
                Wire::Notification(method, params) => {
                    if state.apply(&self.root, &method, &params) {
                        events.push(Event::Changed {
                            status: matches!(method.as_str(), "turn/started" | "turn/completed")
                                .then(|| state.status.clone()),
                        });
                    }
                }
                Wire::Approval { id, method, params } => {
                    let owner = params["threadId"].as_str();
                    if owner == state.dialogue.thread.as_deref()
                        || owner.is_some_and(|id| state.agents.contains_key(id))
                    {
                        let reason = params["reason"].as_str().unwrap_or("Permission required");
                        let command = params["command"].as_str().unwrap_or("");
                        let detail = params["itemId"]
                            .as_str()
                            .and_then(|id| state.approvals.get(id))
                            .cloned()
                            .unwrap_or_default();
                        events.push(Event::Approval {
                            token: id.to_string(),
                            title: format!("{method}\n{reason}\n{command}\n{detail}")
                                .chars()
                                .take(8000)
                                .collect(),
                        });
                    } else {
                        let _ = self
                            .client
                            .send(transport::Command::Approval { id, accept: false });
                    }
                }
                Wire::Voice { id, active } => events.push(Event::Voice { id, active }),
                Wire::VoiceWaiting { id } => events.push(Event::VoiceWaiting { id }),
                Wire::VoiceStarting { id } => events.push(Event::VoiceStarting { id }),
                Wire::Error(error) => events.push(Event::Error(error)),
                Wire::VoiceError(error) => events.push(Event::VoiceError(error)),
                Wire::Closed => events.push(Event::Closed),
            }
        }
        events
    }
}
