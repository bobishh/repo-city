//! Public provider events projected into local dialogue and measured file activity.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Dialogue {
    pub thread: Option<String>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
}

#[derive(Clone)]
pub(crate) struct Presence {
    pub agent: String,
    pub item: String,
    pub path: String,
}

#[derive(Clone)]
pub(crate) struct FileActivity {
    pub serial: u64,
    pub path: String,
    pub line: usize,
}

#[derive(Default)]
pub(crate) struct Projection {
    pub dialogue: Dialogue,
    pub streaming_messages: HashSet<String>,
    pub turn: Option<String>,
    pub presence: Vec<Presence>,
    pub activity: Option<FileActivity>,
    pub agents: HashMap<String, String>,
    pub approvals: HashMap<String, String>,
    pub voice: Option<String>,
    pub status: String,
    pub timeline: super::activity::Timeline,
}

impl Dialogue {
    pub fn message(&mut self, id: &str, role: &str, text: &str, append: bool) {
        if text.is_empty() || (!append && text.trim().is_empty()) {
            return;
        }
        let position = self
            .messages
            .iter()
            .position(|message| message.id == id)
            .or_else(|| {
                if role != "user" || id.starts_with("user-awaiting-") {
                    return None;
                }
                self.messages.iter().rposition(|message| {
                    (message.id.starts_with("user-awaiting-")
                        || message.id.starts_with("user-sent-"))
                        && message.text == text
                })
            });
        if let Some(value) = position.map(|position| &mut self.messages[position]) {
            value.id = id.into();
            if append {
                // Bound a streamed item as well as the complete persisted conversation.
                let available = 64_000_usize.saturating_sub(value.text.len());
                value.text.extend(text.chars().take(available / 4));
            } else {
                value.text = text.chars().take(16_000).collect();
            }
        } else {
            self.messages.push(Message {
                id: id.into(),
                role: role.into(),
                text: text.chars().take(16_000).collect(),
            });
        }
        if self.messages.len() > 160 {
            self.messages.remove(0);
        }
    }
}
