//! Public provider events projected into local dialogue and measured file activity.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Default)]
pub(crate) struct Projection {
    pub dialogue: Dialogue,
    pub turn: Option<String>,
    pub presence: Vec<Presence>,
    pub agents: HashMap<String, String>,
    pub approvals: HashMap<String, String>,
    pub voice: Option<String>,
    pub status: String,
}

impl Projection {
    pub fn message(&mut self, id: &str, role: &str, text: &str, append: bool) {
        if text.trim().is_empty() {
            return;
        }
        let value = self.dialogue.messages.iter_mut().find(|message| {
            message.id == id
                || (role == "user"
                    && message.id.starts_with("user-pending-")
                    && message.text == text)
        });
        if let Some(value) = value {
            value.id = id.into();
            if append {
                // Bound a streamed item as well as the complete persisted conversation.
                let available = 64_000_usize.saturating_sub(value.text.len());
                value.text.extend(text.chars().take(available / 4));
            } else {
                value.text = text.chars().take(16_000).collect();
            }
        } else {
            self.dialogue.messages.push(Message {
                id: id.into(),
                role: role.into(),
                text: text.chars().take(16_000).collect(),
            });
        }
        if self.dialogue.messages.len() > 160 {
            self.dialogue.messages.remove(0);
        }
    }
}
