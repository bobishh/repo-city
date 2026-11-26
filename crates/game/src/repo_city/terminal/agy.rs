//! Interactive Antigravity owns voice and audio while VibePad supplies its terminal.
//! The automatic slash command and empty prompt check follow Ecky b8868ae.
use super::*;

use portable_pty::CommandBuilder;
use serde::Deserialize;
use std::io::Read;

pub(super) const BACKGROUND: [u8; 3] = [7, 13, 23];

const EXPECTED: &str = "REPO_CITY_AGY_CONVERSATION";
const TITLE_EXE: &str = "REPO_CITY_AGY_TITLE_EXE";
const READY: &str = "Repo City Agy ready";
const DIFFERENT: &str = "Repo City Agy different conversation";
const START_VOICE: &[u8] = b"/voice live\r";
const F7: &[u8] = b"\x1b[18~";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum Voice {
    Off,
    Connecting,
    Connected,
}

pub(super) struct Agy {
    pub root: PathBuf,
    process: Process,
    pub requested: bool,
    sent: bool,
    connected: bool,
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
        model: Option<String>,
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
                "colorScheme": "dark",
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
            if let Some(model) = model {
                command.args(["--model", &model]);
            }
            if approval == agent_engine::ApprovalMode::Never {
                command.arg("--dangerously-skip-permissions");
            }
            command.env_remove("GEMINI_API_KEY");
            command.env_remove("REPO_CITY_GEMINI_API_KEY");
            command.env(TITLE_EXE, exe);
            command.env(EXPECTED, conversation);
            Session::open_program(&directory, command, "Antigravity", BACKGROUND).map(Mutex::new)
        });
        Self {
            root,
            process: Process::Starting(job),
            requested: true,
            sent: false,
            connected: false,
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
        if changed {
            let connected = pty
                .screen_rows()
                .filter(|row| !row.trim().is_empty())
                .take(6)
                .any(|row| live_voice_row(&row));
            if self.connected && !connected {
                self.requested = false;
                self.sent = false;
            } else if !self.connected && connected && !self.sent && pty.title != DIFFERENT {
                // The call may have been started with /voice inside the terminal.
                self.requested = true;
                self.sent = true;
            }
            self.connected = connected;
        }
        if let Some(input) =
            voice_input(self.requested, self.sent, &pty.title, pty.at_empty_prompt())
            && pty.send(input.to_vec()).is_ok()
        {
            self.sent = self.requested;
            return true;
        }
        changed
    }

    pub fn voice(&self) -> Voice {
        match (self.requested, self.connected) {
            (_, true) => Voice::Connected,
            (false, false) => Voice::Off,
            (true, false) => Voice::Connecting,
        }
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
            Process::Open(_) => match self.voice() {
                Voice::Connected => "Voice connected · F5 hangs up",
                Voice::Connecting if self.sent => "Connecting voice… · F5 cancels",
                Voice::Connecting => "Waiting for Antigravity · F5 cancels voice start",
                Voice::Off => "Voice off · F5 starts voice",
            },
            Process::Exited(_) => "Antigravity terminal closed",
        }
    }
}

// Antigravity shows this status below the prompt while the CLI call is live.
fn live_voice_row(row: &str) -> bool {
    let mut words = row
        .trim_start_matches(|ch: char| !ch.is_alphanumeric())
        .split_whitespace();
    if words.next() != Some("Live") {
        return false;
    }
    let Some((minutes, seconds)) = words.next().and_then(|time| time.split_once(':')) else {
        return false;
    };
    !minutes.is_empty()
        && minutes.bytes().all(|ch| ch.is_ascii_digit())
        && seconds.len() == 2
        && seconds.bytes().all(|ch| ch.is_ascii_digit())
        && words.next() == Some("·")
        && matches!(
            words.next(),
            Some("listening" | "thinking" | "speaking" | "muted")
        )
}

fn voice_input(
    requested: bool,
    sent: bool,
    title: &str,
    empty_prompt: bool,
) -> Option<&'static [u8]> {
    match (requested, sent) {
        // An idle agent can still have a draft in its terminal editor.
        (true, false) if title == READY && empty_prompt => Some(START_VOICE),
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
    fn native_voice_footer_confirms_a_call_even_when_controls_are_clipped() {
        assert!(live_voice_row(
            "  ● Live 01:53 · speaking · f6 mute · esc hang up"
        ));
        assert!(live_voice_row("● Live 00:00 · listening"));
        assert!(!live_voice_row("  🎤 Started voice chat · 7:31PM"));
        assert!(!live_voice_row("● Live 00:00 · connecting"));
        assert!(!live_voice_row("  Logs: Live 00:00 · listening"));
    }

    #[test]
    fn voice_waits_for_the_exact_idle_conversation_and_hangs_up_while_working() {
        let state = |id: &str, status: &str| TitleState {
            conversation_id: Some(id.into()),
            agent_state: Some(status.into()),
        };
        assert_eq!(
            voice_input(true, false, title(state("other", "idle"), "saved"), true),
            None
        );
        assert_eq!(
            voice_input(true, false, title(state("saved", "working"), "saved"), true),
            None
        );
        assert_eq!(
            voice_input(true, false, title(state("saved", "idle"), "saved"), true),
            Some(b"/voice live\r".as_slice())
        );
        assert_eq!(voice_input(true, false, READY, false), None);
        assert_eq!(voice_input(true, true, READY, true), None);
        assert_eq!(
            voice_input(
                false,
                true,
                title(state("saved", "working"), "saved"),
                false
            ),
            Some(F7)
        );
        assert_eq!(voice_input(false, false, READY, true), None);
    }
}
