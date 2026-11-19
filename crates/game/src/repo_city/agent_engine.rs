//! Agent session contract shared by the city and provider adapters.
//! Providers own authentication and wire protocols; views consume normalized state/events.
mod audio;
mod codex;
mod codex_projection;
mod codex_transport;
mod g711;
mod rtc;
pub(super) mod state;

pub(super) use state::{Dialogue, Projection};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, Copy)]
pub(super) struct Capabilities {
    pub steer: bool,
    pub voice: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub capabilities: Capabilities,
}

pub(super) const CODEX: Provider = Provider {
    id: "codex",
    label: "Codex",
    capabilities: Capabilities {
        steer: true,
        voice: true,
    },
};

pub(super) struct Start {
    pub root: PathBuf,
    pub conversation: Option<String>,
    pub speaking: Arc<AtomicBool>,
    pub diagnostics: Option<PathBuf>,
}

pub(super) enum Command {
    Send { prompt: String, context: String },
    Steer { prompt: String, context: String },
    Voice { id: String, context: String },
    StopVoice,
    Interrupt,
    Approval { token: String, accept: bool },
}

pub(super) enum Event {
    Connected(String),
    Replaced { conversation: String },
    Changed { status: Option<String> },
    Approval { token: String, title: String },
    Voice { id: String, active: bool },
    VoiceWaiting { id: String },
    VoiceStarting { id: String },
    Error(String),
    VoiceError(String),
    Closed,
}

/// A provider updates the shared session projection, then emits UI-neutral lifecycle events.
/// Neither raw provider notifications nor authentication material cross this boundary.
pub(super) trait Engine: Send + Sync {
    fn provider(&self) -> Provider;
    fn send(&self, command: Command) -> Result<(), String>;
    fn poll(&self, state: &mut Projection) -> Vec<Event>;
}

pub(super) fn connect(provider: Provider, start: Start) -> Result<Box<dyn Engine>, String> {
    match provider.id {
        "codex" => Ok(Box::new(codex::Adapter::start(start))),
        _ => Err(format!(
            "Agent provider {} is not installed.",
            provider.label
        )),
    }
}
