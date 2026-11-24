//! Interactive Antigravity owns voice and audio while VibePad supplies its terminal.
use super::*;

use portable_pty::CommandBuilder;
use serde::Deserialize;
use std::io::Read;

const EXPECTED: &str = "REPO_CITY_AGY_CONVERSATION";
const TITLE_EXE: &str = "REPO_CITY_AGY_TITLE_EXE";
const READY: &str = "Repo City Agy ready";
const DIFFERENT: &str = "Repo City Agy different conversation";
const F6: &[u8] = b"\x1b[17~";
const F7: &[u8] = b"\x1b[18~";

pub(super) struct Agy {
    pub root: PathBuf,
    process: Process,
    pub requested: bool,
    sent: bool,
}

enum Process {
    Starting(native_github::Job<Mutex<Session>>),
    Open(Mutex<Session>),
    Exited(Result<(), String>),
}

impl Agy {
    pub fn start(
        root: PathBuf,
        conversation: String,
        approval: agent_engine::ApprovalMode,
    ) -> Self {
        let directory = root.clone();
        let job = native_github::Job::spawn(move || {
            let binary = agent_engine::acp_executable(agent_engine::AGY)
                .ok_or("Install the Antigravity CLI and sign in with Google.")?;
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            let title = if cfg!(windows) {
                "\"%REPO_CITY_AGY_TITLE_EXE%\" --agy-terminal-title"
            } else {
                "exec \"$REPO_CITY_AGY_TITLE_EXE\" --agy-terminal-title"
            };
            let settings = serde_json::json!({
                "title": {"command": title, "type": "command", "enabled": true}
            });
            let mut command = CommandBuilder::new(binary);
            command.args([
                "--sandbox",
                "--conversation",
                &conversation,
                "--settings",
                &settings.to_string(),
            ]);
            if approval == agent_engine::ApprovalMode::Never {
                command.arg("--dangerously-skip-permissions");
            }
            command.env_remove("GEMINI_API_KEY");
            command.env_remove("REPO_CITY_GEMINI_API_KEY");
            command.env(TITLE_EXE, exe);
            command.env(EXPECTED, conversation);
            Session::open_program(&directory, command, "Antigravity").map(Mutex::new)
        });
        Self {
            root,
            process: Process::Starting(job),
            requested: true,
            sent: false,
        }
    }

    pub fn poll(&mut self) -> bool {
        if let Process::Starting(job) = &self.process
            && let Some(result) = job.take()
        {
            self.process = match result {
                Ok(pty) => Process::Open(pty),
                Err(error) => Process::Exited(Err(error)),
            };
        }
        let Process::Open(pty) = &mut self.process else {
            return false;
        };
        let pty = pty.get_mut().expect("Agy terminal session");
        let changed = pty.poll();
        if pty.exited {
            self.process = Process::Exited(Ok(()));
            return true;
        }
        if pty.title == DIFFERENT {
            self.requested = false;
        }
        if let Some(key) = voice_key(self.requested, self.sent, &pty.title)
            && pty.send(key.to_vec()).is_ok()
        {
            self.sent = self.requested;
        }
        changed
    }

    pub fn pty(&mut self) -> Option<&mut Session> {
        match &mut self.process {
            Process::Open(pty) => pty.get_mut().ok(),
            _ => None,
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self.process, Process::Exited(_))
    }

    pub fn outcome(self) -> Result<(), String> {
        match self.process {
            Process::Exited(result) => result,
            _ => Err("The Antigravity terminal is still open.".into()),
        }
    }

    pub fn status(&self) -> &str {
        match &self.process {
            Process::Starting(_) => "Opening Antigravity…",
            Process::Open(pty) if pty.lock().is_ok_and(|pty| pty.title == DIFFERENT) => {
                "Antigravity opened a different conversation. Voice start canceled."
            }
            Process::Open(_) if self.sent => "F5 · Stop Antigravity voice",
            Process::Open(_) if self.requested => {
                "Waiting for Antigravity · F5 cancels voice start"
            }
            Process::Open(_) => "F5 · Start Antigravity voice",
            Process::Exited(_) => "Antigravity terminal closed",
        }
    }
}

fn voice_key(requested: bool, sent: bool, title: &str) -> Option<&'static [u8]> {
    match (requested, sent) {
        (true, false) if title == READY => Some(F6),
        (false, true) => Some(F7),
        _ => None,
    }
}

#[derive(Deserialize)]
struct TitleState {
    conversation_id: Option<String>,
    agent_state: Option<String>,
}

fn title(state: TitleState, expected: &str) -> &'static str {
    match (
        state.conversation_id.as_deref(),
        state.agent_state.as_deref(),
    ) {
        (Some(id), Some("idle")) if id == expected => READY,
        (Some(id), Some("idle")) if id != expected => DIFFERENT,
        _ => "Repo City Agy",
    }
}

pub(crate) fn print_title() {
    let Ok(expected) = std::env::var(EXPECTED) else {
        return;
    };
    if let Ok(state) = serde_json::from_reader(std::io::stdin().lock().take(64 * 1024)) {
        println!("{}", title(state, &expected));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_waits_for_the_exact_idle_conversation_and_hangs_up_while_working() {
        let state = |id: &str, status: &str| TitleState {
            conversation_id: Some(id.into()),
            agent_state: Some(status.into()),
        };
        assert_eq!(
            voice_key(true, false, title(state("other", "idle"), "saved")),
            None
        );
        assert_eq!(
            voice_key(true, false, title(state("saved", "working"), "saved")),
            None
        );
        assert_eq!(
            voice_key(true, false, title(state("saved", "idle"), "saved")),
            Some(F6)
        );
        assert_eq!(voice_key(true, true, READY), None);
        assert_eq!(
            voice_key(false, true, title(state("saved", "working"), "saved")),
            Some(F7)
        );
        assert_eq!(voice_key(false, false, READY), None);
    }
}
