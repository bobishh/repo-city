#[cfg(test)]
use super::state::Dialogue;
use super::state::{Presence, Projection};
use serde_json::Value;
use std::path::Path;
impl Projection {
    fn codex_message(&mut self, id: &str, role: &str, text: &str, append: bool) {
        if !internal(text) {
            self.message(id, role, text, append);
        }
    }

    pub(super) fn apply(&mut self, root: &Path, method: &str, params: &Value) -> bool {
        let Some(thread) = params.get("threadId").and_then(Value::as_str) else {
            return false;
        };
        let primary = self.dialogue.thread.as_deref() == Some(thread);
        if !primary && !self.agents.contains_key(thread) {
            return false;
        }
        match method {
            "turn/started" if primary => {
                self.turn = params
                    .pointer("/turn/id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.status = "Working".into();
            }
            "turn/completed" => {
                self.presence.retain(|presence| presence.agent != thread);
                if primary {
                    self.turn = None;
                    self.status = params
                        .pointer("/turn/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Ready")
                        .chars()
                        .take(1000)
                        .collect();
                }
            }
            "item/agentMessage/delta" => {
                let Some(id) = params.get("itemId").and_then(Value::as_str) else {
                    return false;
                };
                self.codex_message(
                    &format!("{thread}:{id}"),
                    "assistant",
                    params.get("delta").and_then(Value::as_str).unwrap_or(""),
                    true,
                );
            }
            "thread/realtime/item/completed" if primary => {
                let item = &params["item"];
                if item.get("realtimeSessionId").and_then(Value::as_str) != self.voice.as_deref()
                    || self.voice.is_none()
                {
                    return false;
                }
                if item["type"] == "transcriptSegment" {
                    let role = item["role"].as_str().unwrap_or("");
                    if matches!(role, "user" | "assistant") {
                        self.codex_message(
                            &format!("voice:{}", item["id"].as_str().unwrap_or("")),
                            role,
                            item["text"].as_str().unwrap_or(""),
                            false,
                        );
                    }
                }
            }
            "item/started" | "item/completed" => {
                let item = &params["item"];
                let id = item["id"].as_str().unwrap_or("");
                if self.approvals.len() >= 32 {
                    self.approvals.clear();
                }
                if item["type"] == "fileChange" {
                    let detail = item["changes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(12)
                        .map(|change| {
                            format!(
                                "{}\n{}",
                                change["path"].as_str().unwrap_or(""),
                                change["diff"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.approvals
                        .insert(id.into(), detail.chars().take(6000).collect());
                }
                let done = method == "item/completed";
                self.presence
                    .retain(|presence| presence.agent != thread || presence.item != id);
                match item["type"].as_str().unwrap_or("") {
                    "agentMessage" if done => self.codex_message(
                        &format!("{thread}:{id}"),
                        "assistant",
                        item["text"].as_str().unwrap_or(""),
                        false,
                    ),
                    "userMessage" if done && primary => {
                        let text = item["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|part| part["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let text = if text.starts_with("<repo_city_context>") {
                            text.split_once("</repo_city_context>")
                                .map_or(text.as_str(), |(_, text)| text.trim())
                        } else {
                            &text
                        };
                        self.codex_message(&format!("{thread}:{id}"), "user", text, false);
                    }
                    "collabAgentToolCall" if primary => {
                        if let Some(states) = item["agentsStates"].as_object() {
                            for (id, state) in states.iter().take(32) {
                                self.agents.insert(
                                    id.clone(),
                                    state["status"].as_str().unwrap_or("unknown").into(),
                                );
                                if !matches!(
                                    state["status"].as_str(),
                                    Some("running" | "pendingInit")
                                ) {
                                    self.presence.retain(|presence| &presence.agent != id);
                                }
                            }
                        }
                    }
                    kind if !done => {
                        let paths: Vec<&str> = match kind {
                            "fileChange" => item["changes"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|change| change["path"].as_str())
                                .collect(),
                            "commandExecution" => item["commandActions"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter(|action| action["type"] == "read")
                                .filter_map(|action| action["path"].as_str())
                                .collect(),
                            "mcpToolCall" => item
                                .pointer("/arguments/path")
                                .and_then(Value::as_str)
                                .into_iter()
                                .collect(),
                            _ => Vec::new(),
                        };
                        for path in paths
                            .into_iter()
                            .take(16)
                            .filter_map(|path| relative(root, path))
                        {
                            self.presence.push(Presence {
                                agent: thread.into(),
                                item: id.into(),
                                path,
                            });
                        }
                    }
                    _ => {}
                }
            }
            _ => return false,
        }
        true
    }
}

fn relative(root: &Path, path: &str) -> Option<String> {
    let path = Path::new(path);
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    path.to_str().map(str::to_owned)
}

// Adapted from Ecky's complete-envelope filter; native internal voice handoffs are not dialogue.
fn internal(text: &str) -> bool {
    let text = text.trim();
    text.starts_with("<realtime_delegation>") && text.ends_with("</realtime_delegation>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn owned_dialogue_keeps_complete_voice_text_and_rejects_stale_sessions() {
        let mut view = projection();
        view.voice = Some("voice-2".into());
        let event = |session: &str| json!({"threadId":"main","item":{"id":"line-1","type":"transcriptSegment","role":"user","text":"Explain this code","realtimeSessionId":session}});
        assert!(!view.apply(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-1")
        ));
        assert!(view.apply(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-2")
        ));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert_eq!(view.dialogue.messages[0].text, "Explain this code");
        view.apply(
            Path::new("/repo"),
            "thread/realtime/item/completed",
            &event("voice-2"),
        );
        assert_eq!(view.dialogue.messages.len(), 1);
    }

    #[test]
    fn presence_uses_explicit_tool_paths_and_owned_child_identity_only() {
        let mut view = projection();
        let item = json!({"threadId":"child","item":{"id":"read-1","type":"commandExecution","commandActions":[{"type":"read","path":"/repo/src/main.rs"}]}});
        assert!(!view.apply(Path::new("/repo"), "item/started", &item));
        view.apply(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"spawn-1","type":"collabAgentToolCall","receiverThreadIds":["child"],"agentsStates":{"child":{"status":"running"}}}}));
        assert!(view.apply(Path::new("/repo"), "item/started", &item));
        assert_eq!(view.presence[0].path, "src/main.rs");
        assert_eq!(view.presence[0].agent, "child");
        view.apply(Path::new("/repo"), "item/completed", &item);
        assert!(view.presence.is_empty());
        view.apply(Path::new("/repo"), "item/started", &json!({"threadId":"main","item":{"id":"escape","type":"fileChange","changes":[{"path":"../other/secrets"}]}}));
        assert!(view.presence.is_empty());
    }

    #[test]
    fn final_item_replaces_streamed_text_and_internal_voice_envelope_is_hidden() {
        let mut view = projection();
        for delta in ["Hello", " there"] {
            view.apply(
                Path::new("/repo"),
                "item/agentMessage/delta",
                &json!({"threadId":"main","itemId":"a","delta":delta}),
            );
        }
        view.apply(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"a","type":"agentMessage","text":"Hello there"}}));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert_eq!(view.dialogue.messages[0].text, "Hello there");
        view.apply(Path::new("/repo"), "item/completed", &json!({"threadId":"main","item":{"id":"internal","type":"userMessage","content":[{"type":"text","text":"<realtime_delegation><input>hidden</input></realtime_delegation>"}]}}));
        assert_eq!(view.dialogue.messages.len(), 1);
        assert!(!view.apply(
            Path::new("/repo"),
            "item/agentMessage/delta",
            &json!({"threadId":"different","itemId":"x","delta":"Wrong repo"})
        ));
    }
}
