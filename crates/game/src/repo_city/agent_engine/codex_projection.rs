//! Reduces validated Codex events into the city session state.
use super::activity::{Details, Kind, Phase};
use super::codex_protocol::{Item, Location, Notification, Role, ThreadPhase, Update};
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
            Update::TurnStarted { id } => {
                self.timeline.record(
                    &thread,
                    &format!("turn:{id}"),
                    Details::new(Kind::Session, "Turn started"),
                    Phase::Running,
                );
                if primary {
                    self.turn = Some(id);
                    self.status = "Working".into();
                }
            }
            Update::TurnCompleted {
                id,
                error,
                interrupted,
            } => {
                if primary && self.turn.as_deref().is_some_and(|active| active != id) {
                    return false;
                }
                self.presence.retain(|presence| presence.agent != thread);
                self.timeline.end_turn(&thread);
                let (title, phase) = if interrupted {
                    ("Work stopped", Phase::Ended)
                } else if error.is_some() {
                    ("Turn failed", Phase::Failed)
                } else {
                    ("Turn completed", Phase::Completed)
                };
                let mut details = Details::new(Kind::Session, title);
                details.body = error.clone().unwrap_or_default();
                self.timeline
                    .record(&thread, &format!("turn:{id}"), details, phase);
                if primary {
                    self.turn = None;
                    self.streaming_messages.clear();
                    self.status = if interrupted {
                        "Work stopped".into()
                    } else {
                        error.unwrap_or_else(|| "Ready".into())
                    }
                    .chars()
                    .take(1000)
                    .collect();
                }
            }
            Update::ActivityDelta { id, kind, text } => {
                self.timeline.append(&thread, &id, kind, &text)
            }
            Update::Activity { id, details, phase } => {
                self.timeline.record(&thread, &id, details, phase)
            }
            Update::ThreadStatus { status } => {
                let (title, phase) = match status {
                    ThreadPhase::Active => ("Thread active", Phase::Completed),
                    ThreadPhase::Idle => ("Thread idle", Phase::Completed),
                    ThreadPhase::NotLoaded => ("Thread unloaded", Phase::Ended),
                    ThreadPhase::SystemError => ("Thread reported a system error", Phase::Failed),
                    ThreadPhase::Other => return false,
                };
                self.timeline.record(
                    &thread,
                    "thread-status",
                    Details::new(Kind::Session, title),
                    phase,
                );
            }
            Update::MessageDelta { id, text } => {
                let id = format!("{thread}:{id}");
                dialogue_message(&mut self.dialogue, &id, Role::Assistant, &text, true);
                self.streaming_messages.insert(id);
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
                let id = format!("{thread}:{id}");
                self.streaming_messages.remove(&id);
                dialogue_message(&mut self.dialogue, &id, role, &text, false);
            }
            Item::Edit {
                files,
                applied,
                detail,
            } => {
                let mut activity = Details::new(
                    Kind::Edit,
                    if files.is_empty() {
                        "File changes"
                    } else {
                        "Edit repository files"
                    },
                );
                activity.body = detail.clone();
                activity.locations = files.clone();
                self.timeline
                    .record(thread, id, activity, phase(completed, !applied));
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
            Item::Read {
                files,
                mut activity,
            } => {
                activity.locations = files.clone();
                let phase = phase(completed, activity.failed);
                self.timeline.record(thread, id, activity, phase);
                if !completed {
                    self.present_files(thread, id, files, true);
                }
            }
            Item::Collaboration { agents, tool } => {
                let mut activity = Details::new(
                    Kind::Delegate,
                    tool.unwrap_or_else(|| "Subagent activity".into()),
                );
                let mut status = agents
                    .iter()
                    .map(|(id, state)| format!("{id} · {}", state.status.as_str()))
                    .collect::<Vec<_>>();
                status.sort_unstable();
                activity.body = status.into_iter().take(16).collect::<Vec<_>>().join("\n");
                self.timeline
                    .record(thread, id, activity, phase(completed, false));
                if primary {
                    for (id, state) in agents.into_iter().take(32) {
                        if !state.status.active() {
                            self.presence.retain(|presence| presence.agent != id);
                        }
                        self.agents.insert(id, state.status.as_str().into());
                    }
                }
            }
            Item::Activity(activity) => {
                self.timeline
                    .record(thread, id, activity, phase(completed, false))
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

fn phase(completed: bool, failed: bool) -> Phase {
    match (completed, failed) {
        (false, _) => Phase::Running,
        (true, false) => Phase::Completed,
        (true, true) => Phase::Failed,
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
    fn interrupted_turn_is_reported_as_stopped_without_leaving_any_live_items() {
        let root = Path::new("/repo");
        let mut view = projection();
        assert!(view.apply_wire(
            root,
            "turn/started",
            &json!({"threadId":"main","turn":{"id":"active"}})
        ));
        assert!(view.apply_wire(
            root,
            "item/started",
            &json!({"threadId":"main","item":{"id":"running","type":"plan","text":"Work"}})
        ));
        assert!(view.apply_wire(
            root,
            "turn/completed",
            &json!({"threadId":"main","turn":{"id":"active","status":"interrupted","error":null}})
        ));
        assert_eq!(view.status, "Work stopped");
        assert!(view.turn.is_none());
        assert!(
            view.timeline
                .entries
                .iter()
                .all(|item| item.phase == Phase::Ended)
        );
    }

    #[test]
    fn one_live_command_keeps_its_file_anchor_and_ignores_unrelated_threads() {
        let root = Path::new("/repo");
        let mut view = projection();
        assert!(!view.apply_wire(
            root,
            "item/commandExecution/outputDelta",
            &json!({"threadId":"unrelated","itemId":"cmd","delta":"private"})
        ));
        assert!(view.apply_wire(root, "item/started", &json!({"threadId":"main","item":{"id":"cmd","type":"commandExecution","command":"cat example.rs","commandActions":[{"type":"read","path":"example.rs"}]}})));
        assert!(view.apply_wire(
            root,
            "item/commandExecution/outputDelta",
            &json!({"threadId":"main","itemId":"cmd","delta":"first line\n"})
        ));
        assert_eq!(view.timeline.entries[0].details.body, "first line\n");
        let serial = view.timeline.entries[0].serial;
        assert!(view.apply_wire(root, "item/completed", &json!({"threadId":"main","item":{"id":"cmd","type":"commandExecution","command":"cat example.rs","exitCode":0}})));
        assert_eq!(view.timeline.entries.len(), 1);
        let item = &view.timeline.entries[0];
        assert_eq!(item.serial, serial);
        assert_eq!(item.phase, Phase::Completed);
        assert_eq!(item.details.body, "first line\n");
        assert_eq!(item.details.locations[0].path, "example.rs");
        assert_eq!(item.details.locations[0].line, 1);
    }

    #[test]
    fn a_reported_child_can_stream_a_file_read_without_a_legacy_spawn_call() {
        let root = Path::new("/repo");
        let mut view = projection();
        assert!(view.apply_wire(
            root,
            "item/started",
            &json!({"threadId":"main","item":{
                "id":"delegate","type":"subAgentActivity","agentThreadId":"child","kind":"started"
            }})
        ));
        assert!(view.apply_wire(
            root,
            "item/started",
            &json!({"threadId":"child","item":{
                "id":"read","type":"imageView","path":"diagram.png"
            }})
        ));
        let read = view.timeline.entries.back().unwrap();
        assert_eq!(read.thread, "child");
        assert_eq!(read.details.locations[0].path, "diagram.png");
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
        assert!(view.streaming_messages.contains("main:a"));
        view.apply_wire(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"a","type":"agentMessage","text":"Hello there"}}));
        assert!(view.streaming_messages.is_empty());
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
