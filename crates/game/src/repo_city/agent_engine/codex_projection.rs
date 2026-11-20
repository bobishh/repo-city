//! Reduces validated Codex events into the city session state.
use super::codex_protocol::{Item, Location, Notification, Role, Update};
use super::state::{Dialogue, FileActivity, Presence, Projection};

impl Projection {
    pub(super) fn owns_thread(&self, thread: &str) -> bool {
        self.dialogue.thread.as_deref() == Some(thread) || self.agents.contains_key(thread)
    }

    pub(super) fn apply(&mut self, event: Notification) -> bool {
        let thread = event.thread;
        if !self.owns_thread(&thread) {
            return false;
        }
        let primary = self.dialogue.thread.as_deref() == Some(thread.as_str());
        match event.update {
            Update::TurnStarted { id } if primary => {
                self.turn = Some(id);
                self.status = "Working".into();
            }
            Update::TurnStarted { .. } => return false,
            Update::TurnCompleted { error } => {
                self.presence.retain(|presence| presence.agent != thread);
                if primary {
                    self.turn = None;
                    self.status = error
                        .unwrap_or_else(|| "Ready".into())
                        .chars()
                        .take(1000)
                        .collect();
                }
            }
            Update::MessageDelta { id, text } => {
                dialogue_message(
                    &mut self.dialogue,
                    &format!("{thread}:{id}"),
                    Role::Assistant,
                    &text,
                    true,
                );
            }
            Update::Transcript {
                session,
                id,
                role,
                text,
            } => {
                if !primary || self.voice.as_deref() != Some(session.as_str()) {
                    return false;
                }
                dialogue_message(
                    &mut self.dialogue,
                    &format!("voice:{id}"),
                    role,
                    &text,
                    false,
                );
            }
            Update::Item {
                id,
                completed,
                content,
            } => self.item(&thread, &id, primary, completed, content),
        }
        true
    }

    fn item(&mut self, thread: &str, id: &str, primary: bool, completed: bool, content: Item) {
        self.presence
            .retain(|presence| presence.agent != thread || presence.item != id);
        match content {
            Item::Message { role, text }
                if completed && (primary || matches!(role, Role::Assistant)) =>
            {
                dialogue_message(
                    &mut self.dialogue,
                    &format!("{thread}:{id}"),
                    role,
                    &text,
                    false,
                );
            }
            Item::Edit {
                files,
                applied,
                detail,
            } => {
                if self.approvals.len() >= 32 {
                    self.approvals.clear();
                }
                self.approvals.insert(id.into(), detail);
                if completed && applied {
                    for file in files {
                        self.file_activity(file);
                    }
                } else if !completed {
                    self.present_files(thread, id, files, false);
                }
            }
            Item::Read { files } if !completed => self.present_files(thread, id, files, true),
            Item::Collaboration { agents } if primary => {
                for (id, state) in agents.into_iter().take(32) {
                    if !state.status.active() {
                        self.presence.retain(|presence| presence.agent != id);
                    }
                    self.agents.insert(id, state.status.as_str().into());
                }
            }
            _ => {}
        }
    }

    fn present_files(&mut self, thread: &str, id: &str, files: Vec<Location>, follow: bool) {
        for file in files {
            self.presence.push(Presence {
                agent: thread.into(),
                item: id.into(),
                path: file.path.clone(),
            });
            if follow {
                self.file_activity(file);
            }
        }
    }

    fn file_activity(&mut self, file: Location) {
        let serial = self
            .activity
            .as_ref()
            .map_or(1, |activity| activity.serial.wrapping_add(1));
        self.activity = Some(FileActivity {
            serial,
            path: file.path,
            line: file.line,
        });
    }
}

fn dialogue_message(dialogue: &mut Dialogue, id: &str, role: Role, text: &str, append: bool) {
    // Internal voice handoffs are envelopes, not user-visible dialogue.
    let trimmed = text.trim();
    if !(trimmed.starts_with("<realtime_delegation>")
        && trimmed.ends_with("</realtime_delegation>"))
    {
        dialogue.message(id, role.as_str(), text, append);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::path::Path;

    impl Projection {
        fn apply_wire(&mut self, root: &Path, method: &str, params: &Value) -> bool {
            match super::super::codex_protocol::notification(root, method, params.clone()) {
                Ok(Some(event)) => self.apply(event),
                Ok(None) | Err(_) => false,
            }
        }
    }

    fn projection() -> Projection {
        Projection {
            dialogue: Dialogue {
                thread: Some("main".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn malformed_supported_notifications_do_not_mutate_session_state() {
        // Given an owned conversation, malformed supported events must not look like empty data.
        for (method, params) in [
            ("turn/started", json!({"threadId":"main","turn":{}})),
            (
                "item/agentMessage/delta",
                json!({"threadId":"main","itemId":"a"}),
            ),
            (
                "item/completed",
                json!({"threadId":"main","item":{"type":"agentMessage","text":"missing ID"}}),
            ),
            (
                "item/started",
                json!({"threadId":"main","item":{"id":"edit","type":"fileChange","status":"inProgress","changes":{}}}),
            ),
        ] {
            let mut view = projection();
            assert!(
                !view.apply_wire(Path::new("/repo"), method, &params),
                "accepted malformed {method}"
            );
            assert!(view.turn.is_none());
            assert!(view.dialogue.messages.is_empty());
            assert!(view.approvals.is_empty());
            assert!(view.presence.is_empty());
        }
    }

    #[test]
    fn owned_dialogue_keeps_complete_voice_text_and_rejects_stale_sessions() {
        let mut view = projection();
        view.voice = Some("voice-2".into());
        let event = |session: &str| json!({"threadId":"main","item":{"id":"line-1","type":"transcriptSegment","role":"user","text":"Explain this code","realtimeSessionId":session}});
        assert!(!view.apply_wire(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-1")
        ));
        assert!(view.apply_wire(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-2")
        ));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert_eq!(view.dialogue.messages[0].text, "Explain this code");
        view.apply_wire(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-2"),
        );
        assert_eq!(view.dialogue.messages.len(), 1);
    }

    #[test]
    fn edited_file_activity_waits_for_completion_and_keeps_the_patch_anchor() {
        let mut view = projection();
        let mut item = json!({"threadId":"main", "item":{"id":"edit", "type":"fileChange", "status":"completed", "changes":[{"path":"/repo/src/main.rs", "diff":"@@ -4,2 +7,3 @@\n+let value = 1;"}]}});
        view.apply_wire(Path::new("/repo"), "item/started", &item);
        assert!(view.activity.is_none());
        view.apply_wire(Path::new("/repo"), "item/completed", &item);
        let activity = view.activity.as_ref().unwrap();
        assert_eq!((&*activity.path, activity.line), ("src/main.rs", 7));
        let serial = activity.serial;
        for status in ["failed", "declined"] {
            item["item"]["status"] = json!(status);
            view.apply_wire(Path::new("/repo"), "item/completed", &item);
            assert_eq!(view.activity.as_ref().unwrap().serial, serial);
        }
    }

    #[test]
    fn presence_uses_explicit_tool_paths_and_owned_child_identity_only() {
        let mut view = projection();
        let item = json!({"threadId":"child","item":{"id":"read-1","type":"commandExecution","commandActions":[{"type":"read","path":"/repo/src/main.rs"}]}});
        assert!(!view.apply_wire(Path::new("/repo"), "item/started", &item));
        view.apply_wire(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"spawn-1","type":"collabAgentToolCall","receiverThreadIds":["child"],"agentsStates":{"child":{"status":"running"}}}}));
        assert!(view.apply_wire(Path::new("/repo"), "item/started", &item));
        assert_eq!(view.presence[0].path, "src/main.rs");
        assert_eq!(view.presence[0].agent, "child");
        view.apply_wire(Path::new("/repo"), "item/completed", &item);
        assert!(view.presence.is_empty());
        view.apply_wire(Path::new("/repo"), "item/started", &json!({"threadId":"main","item":{"id":"escape","type":"fileChange","status":"inProgress","changes":[{"path":"../other/secrets","diff":""}]}}));
        assert!(view.presence.is_empty());
    }

    #[test]
    fn final_item_replaces_streamed_text_and_internal_voice_envelope_is_hidden() {
        let mut view = projection();
        for delta in ["Hello", " there"] {
            view.apply_wire(
                Path::new("/repo"),
                "item/agentMessage/delta",
                &json!({"threadId":"main","itemId":"a","delta":delta}),
            );
        }
        view.apply_wire(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"a","type":"agentMessage","text":"Hello there"}}));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert_eq!(view.dialogue.messages[0].text, "Hello there");
        view.apply_wire(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"internal","type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>hidden</input></realtime_delegation>"}]}}));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert!(!view.apply_wire(
            Path::new("/repo"),
            "item/agentMessage/delta",
            &json!({"threadId":"different","itemId":"x","delta":"Wrong repo"})
        ));
    }
}
