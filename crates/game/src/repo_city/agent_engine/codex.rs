//! Codex adapter: maps app-server messages into the shared agent session contract.
use super::{Approval, CODEX, Command, Engine, Event, Projection, Provider, Start};
use super::{codex_protocol as protocol, codex_transport as transport};
use std::path::{Path, PathBuf};

pub(super) struct Adapter {
    root: PathBuf,
    client: transport::Client,
}
impl Adapter {
    pub fn start(start: Start) -> Self {
        let root = start.root.clone();
        Self {
            client: transport::Client::start(start),
            root,
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
            Command::RefreshModels => Wire::RefreshModels,
            Command::SelectModel(model) => Wire::SelectModel(model),
            Command::Send(prompt) => Wire::Send(prompt),
            Command::Steer(prompt) => Wire::Steer(prompt),
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
                Wire::Models(models) => events.push(Event::Models(models)),
                Wire::Connected(id) => {
                    state.dialogue.thread = Some(id.clone());
                    events.push(Event::Connected(id));
                }
                Wire::Replaced {
                    thread,
                    writer_conflict,
                } => events.push(Event::Replaced {
                    conversation: thread,
                    writer_conflict,
                }),
                Wire::Delivery { id, result } => events.push(Event::Delivery { id, result }),
                Wire::Notification(method, params) => {
                    if let Some(event) = apply_notification(&self.root, state, &method, params) {
                        events.push(event);
                    }
                }
                Wire::Approval { id, method, params } => match protocol::approval(params) {
                    Ok(approval) if state.owns_thread(&approval.thread_id) => {
                        events.push(Event::Approval(approval_request(
                            id.to_string(),
                            &method,
                            approval,
                            state,
                        )));
                    }
                    rejected => {
                        let _ = self
                            .client
                            .send(transport::Command::Approval { id, accept: false });
                        if let Err(error) = rejected {
                            events.push(Event::Error(error));
                        }
                    }
                },
                Wire::Voice { id, active } => events.push(Event::Voice { id, active }),
                Wire::VoiceWaiting { id } => events.push(Event::VoiceWaiting { id }),
                Wire::VoiceStarting { id } => events.push(Event::VoiceStarting { id }),
                Wire::StopUnconfirmed(error) => events.push(Event::StopUnconfirmed(error)),
                Wire::Error(error) => events.push(Event::Error(error)),
                Wire::VoiceError(error) => events.push(Event::VoiceError(error)),
                Wire::Closed => events.push(Event::Closed),
            }
        }
        events
    }
}

fn approval_request(
    token: String,
    method: &str,
    approval: protocol::Approval,
    state: &Projection,
) -> Approval {
    let files = method == "item/fileChange/requestApproval";
    let fallback = if files {
        "Allow these file changes?"
    } else {
        "Allow this command?"
    };
    let mut summary = approval
        .reason
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or_else(|| fallback.into());
    if let Some(detail) = state
        .approvals
        .get(&approval.item_id)
        .filter(|s| !s.is_empty())
    {
        summary.push_str("\n\n");
        summary.push_str(detail);
    }
    Approval {
        token,
        summary,
        command: approval.command.filter(|command| !command.is_empty()),
    }
}

fn apply_notification(
    root: &Path,
    state: &mut Projection,
    method: &str,
    params: serde_json::Value,
) -> Option<Event> {
    match protocol::notification(root, method, params) {
        Ok(Some(notification)) => {
            let status_changed = state.dialogue.thread.as_deref()
                == Some(notification.thread.as_str())
                && matches!(
                    notification.update,
                    protocol::Update::TurnStarted { .. } | protocol::Update::TurnCompleted { .. }
                );
            state.apply(notification).then(|| Event::Changed {
                status: status_changed.then(|| state.status.clone()),
            })
        }
        Ok(None) => None,
        Err(error) => Some(Event::Error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_supported_notification_reports_an_error_without_changing_state() {
        let mut state = Projection::default();
        state.dialogue.thread = Some("main".into());
        let event = apply_notification(
            Path::new("/repo"),
            &mut state,
            "turn/started",
            json!({"threadId":"main","turn":{}}),
        );
        assert!(
            matches!(event, Some(Event::Error(error)) if error.contains("turn/started") && error.contains("id"))
        );
        assert!(state.turn.is_none());
        assert!(state.status.is_empty());
        assert!(
            apply_notification(Path::new("/repo"), &mut state, "future/event", json!({})).is_none()
        );
    }
}
