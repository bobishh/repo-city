//! Native speaking companion for the existing Antigravity ACP conversation.
mod transport;

use super::{AGY, Command, Engine, Event, Projection, Prompt, Provider};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::TryRecvError,
    },
};
use transport::{Call, Transport, Update};

pub(super) struct Adapter {
    inner: Box<dyn Engine>,
    state: Mutex<State>,
    speaking: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    connected: bool,
    voice: Option<Bridge>,
}

struct Bridge {
    id: String,
    context: String,
    transport: Transport,
    next: u64,
    seen: HashSet<String>,
    pending: HashMap<String, Pending>,
}

struct Pending {
    call: Call,
    canceled: bool,
}

impl Adapter {
    pub(super) fn new(inner: Box<dyn Engine>, speaking: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            speaking,
            state: Mutex::default(),
        }
    }
}

impl Drop for Adapter {
    fn drop(&mut self) {
        self.speaking.store(false, Ordering::Relaxed);
    }
}

impl Engine for Adapter {
    fn provider(&self) -> Provider {
        AGY
    }

    fn send(&self, command: Command) -> Result<(), String> {
        match command {
            Command::Voice { id, context } => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "Antigravity voice state is unavailable.")?;
                if state.voice.is_some() {
                    return Err("Antigravity voice is already active.".into());
                }
                let camera = serde_json::from_str::<Value>(&context)
                    .ok()
                    .and_then(|value| value["camera"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                state.voice = Some(Bridge {
                    id,
                    context: camera,
                    transport: Transport::start(),
                    next: 0,
                    seen: HashSet::new(),
                    pending: HashMap::new(),
                });
                Ok(())
            }
            Command::StopVoice => {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "Antigravity voice state is unavailable.")?;
                state.voice = None;
                self.speaking.store(false, Ordering::Relaxed);
                Ok(())
            }
            other => self.inner.send(other),
        }
    }

    fn poll(&self, projection: &mut Projection) -> Vec<Event> {
        let mut events = self.inner.poll(projection);
        let Ok(mut state) = self.state.lock() else {
            return events;
        };
        let closed = events.iter().any(|event| matches!(event, Event::Closed));
        if closed {
            state.connected = false;
            state.voice = None;
        } else if events
            .iter()
            .any(|event| matches!(event, Event::Connected(_)))
        {
            state.connected = true;
        }
        let connected = state.connected;
        if let Some(voice) = &mut state.voice {
            match voice.poll(self.inner.as_ref(), connected, projection, &mut events) {
                Ok(()) => self.speaking.store(
                    voice.transport.speaking.load(Ordering::Relaxed),
                    Ordering::Relaxed,
                ),
                Err(error) => {
                    events.push(Event::Voice {
                        id: voice.id.clone(),
                        active: false,
                    });
                    events.push(Event::VoiceError(error));
                    state.voice = None;
                    self.speaking.store(false, Ordering::Relaxed);
                }
            }
        } else {
            self.speaking.store(false, Ordering::Relaxed);
        }
        events.retain(|event| !matches!(event, Event::WorkFinished { .. }));
        events
    }
}

impl Bridge {
    fn poll(
        &mut self,
        engine: &dyn Engine,
        connected: bool,
        projection: &mut Projection,
        events: &mut Vec<Event>,
    ) -> Result<(), String> {
        for event in events.iter() {
            match event {
                Event::Delivery { id, result } => {
                    let finishing = self.pending.get(id).is_some_and(|pending| {
                        pending.call.name == "steer_work" || result.is_err()
                    });
                    if finishing
                        && let Some(Pending {
                            call,
                            canceled: false,
                        }) = self.pending.remove(id)
                    {
                        let result = match result {
                            Ok(()) => json!({"state":"accepted"}),
                            Err(error) => json!({"state":"rejected", "error":bounded(error, 400)}),
                        };
                        self.transport.reply(&call.id, &call.name, result)?;
                    }
                }
                Event::WorkFinished {
                    id,
                    result,
                    stopped,
                } => {
                    if let Some(Pending {
                        call,
                        canceled: false,
                    }) = self.pending.remove(id)
                    {
                        let result = match result {
                            Err(error) => json!({"state":"failed", "error":bounded(error, 400)}),
                            Ok(()) => {
                                json!({"state": if *stopped { "canceled" } else { "finished" }, "reply": answer(projection, id)})
                            }
                        };
                        self.transport.reply(&call.id, &call.name, result)?;
                    }
                }
                _ => {}
            }
        }
        for _ in 0..32 {
            match self.transport.events.try_recv() {
                Ok(Update::Ready) => events.push(Event::Voice {
                    id: self.id.clone(),
                    active: true,
                }),
                Ok(Update::Call(call)) => self.call(engine, connected, projection, call)?,
                Ok(Update::Canceled(id)) => {
                    if let Some(pending) = self
                        .pending
                        .values_mut()
                        .find(|pending| pending.call.id == id)
                    {
                        pending.canceled = true;
                    }
                }
                Ok(Update::Transcript { assistant, text }) => {
                    self.next += 1;
                    projection.dialogue.message(
                        &format!("voice:{}:{}", self.id, self.next),
                        if assistant { "assistant" } else { "user" },
                        &text,
                        false,
                    );
                    events.push(Event::Changed { status: None });
                }
                Ok(Update::Ended(error)) => return Err(error),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Err("Gemini Live voice stopped.".into()),
            }
        }
        Ok(())
    }

    fn call(
        &mut self,
        engine: &dyn Engine,
        connected: bool,
        projection: &Projection,
        call: Call,
    ) -> Result<(), String> {
        if call.id.is_empty() || call.id.len() > 256 || call.name.len() > 64 {
            return Err("Gemini Live returned an invalid work request.".into());
        }
        if self.seen.len() >= 1024 {
            return Err("Gemini Live reached this call's work-request limit. Start voice again to continue.".into());
        }
        if !self.seen.insert(call.id.clone()) {
            return Ok(());
        }
        let result = self.dispatch(engine, connected, projection, &call);
        match result {
            Ok(Some(id)) => {
                self.pending.insert(
                    id,
                    Pending {
                        call,
                        canceled: false,
                    },
                );
                Ok(())
            }
            Ok(None) => self.transport.reply(
                &call.id,
                &call.name,
                status(projection, self.start_pending(), call.name == "stop_work"),
            ),
            Err(error) => self.transport.reply(
                &call.id,
                &call.name,
                json!({"state":"rejected", "error":bounded(&error, 400)}),
            ),
        }
    }

    fn dispatch(
        &mut self,
        engine: &dyn Engine,
        connected: bool,
        projection: &Projection,
        call: &Call,
    ) -> Result<Option<String>, String> {
        if !connected {
            return Err("Wait for the Antigravity conversation to connect.".into());
        }
        match call.name.as_str() {
            "work_status" => return Ok(None),
            "stop_work" => {
                if projection.turn.is_none() && !self.start_pending() {
                    return Err("There is no Antigravity task to stop.".into());
                }
                engine.send(Command::Interrupt)?;
                return Ok(None);
            }
            "start_work" | "steer_work" => {}
            _ => return Err("This voice tool is unavailable.".into()),
        }
        let steer = call.name == "steer_work";
        let active = projection.turn.is_some();
        if steer && !active {
            return Err("There is no active Antigravity task to steer.".into());
        }
        if !steer && (active || self.start_pending()) {
            return Err(
                "Antigravity is already working. Use steer_work to change the running task.".into(),
            );
        }
        if self.pending.len() >= 5 {
            return Err("Wait for Antigravity to acknowledge the pending corrections.".into());
        }
        let task = call.args["task"].as_str().unwrap_or_default().trim();
        if task.is_empty() || task.chars().count() > 4000 {
            return Err("Provide a work request from 1 to 4000 characters.".into());
        }
        self.next += 1;
        let id = format!("voice-work:{}:{}", self.id, self.next);
        let prompt = Prompt {
            id: id.clone(),
            text: task.into(),
            context: self.context.clone(),
        };
        engine.send(if steer {
            Command::Steer(prompt)
        } else {
            Command::Send(prompt)
        })?;
        Ok(Some(id))
    }

    fn start_pending(&self) -> bool {
        self.pending
            .values()
            .any(|pending| pending.call.name == "start_work")
    }
}

fn bounded(text: &str, length: usize) -> String {
    text.chars().take(length).collect()
}

fn answer(projection: &Projection, prompt: &str) -> String {
    let assistant = format!("assistant:{prompt}");
    projection
        .dialogue
        .messages
        .iter()
        .rev()
        .find(|message| message.id == assistant)
        .map_or_else(String::new, |message| bounded(&message.text, 2000))
}

fn status(projection: &Projection, queued: bool, canceling: bool) -> Value {
    if canceling {
        return json!({"state":"stop_requested"});
    }
    if let Some(turn) = &projection.turn {
        let activity = projection
            .timeline
            .entries
            .iter()
            .rev()
            .find(|entry| {
                Some(entry.thread.as_str()) == projection.dialogue.thread.as_deref()
                    && entry.phase == super::activity::Phase::Running
                    && entry.details.kind != super::activity::Kind::Session
            })
            .map(|entry| entry.details.kind.label());
        return json!({"state":"working", "activity":activity, "reply_so_far":answer(projection, turn)});
    }
    if queued {
        return json!({"state":"queued"});
    }
    let latest = projection
        .dialogue
        .messages
        .iter()
        .rev()
        .find(|message| message.id.starts_with("assistant:"))
        .map_or_else(String::new, |message| bounded(&message.text, 2000));
    json!({"state":"idle", "last_reply":latest})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Acp {
        commands: Arc<Mutex<Vec<Command>>>,
        events: Arc<Mutex<Vec<Event>>>,
    }

    impl Engine for Acp {
        fn provider(&self) -> Provider {
            AGY
        }
        fn send(&self, command: Command) -> Result<(), String> {
            self.commands.lock().unwrap().push(command);
            Ok(())
        }
        fn poll(&self, _: &mut Projection) -> Vec<Event> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
    }

    fn session(transport: Transport) -> Bridge {
        Bridge {
            id: "call".into(),
            context: "current checkout".into(),
            transport,
            next: 0,
            seen: HashSet::new(),
            pending: HashMap::new(),
        }
    }

    fn call(id: &str, name: &str) -> Update {
        Update::Call(Call {
            id: id.into(),
            name: name.into(),
            args: json!({"task":"Review the spec"}),
        })
    }

    #[test]
    fn conversation_can_check_progress_while_the_same_acp_turn_runs() {
        let acp = Acp::default();
        let commands = acp.commands.clone();
        let events = acp.events.clone();
        let engine = Adapter::new(Box::new(acp), Arc::default());
        let (transport, from_gemini, to_gemini) = Transport::fixture();
        engine.state.lock().unwrap().voice = Some(session(transport));
        events
            .lock()
            .unwrap()
            .push(Event::Connected("saved ACP session".into()));
        let mut state = Projection::default();
        from_gemini.send(call("work", "start_work")).unwrap();
        from_gemini.send(call("work", "start_work")).unwrap();
        engine.poll(&mut state);
        let mut pending = commands.lock().unwrap();
        assert_eq!(pending.len(), 1);
        let Command::Send(prompt) = pending.pop().unwrap() else {
            panic!("expected an ACP prompt");
        };
        assert_eq!(prompt.context, "current checkout");
        assert!(to_gemini.try_recv().is_err());
        drop(pending);
        state.turn = Some(prompt.id.clone());
        let reply_id = format!("assistant:{}", prompt.id);
        state
            .dialogue
            .message(&reply_id, "assistant", "Inspecting the spec", false);
        from_gemini.send(call("progress", "work_status")).unwrap();
        engine.poll(&mut state);
        let progress = to_gemini.try_recv().unwrap();
        assert_eq!(
            progress["toolResponse"]["functionResponses"][0]["response"]["reply_so_far"],
            "Inspecting the spec"
        );
        assert!(commands.lock().unwrap().is_empty());
        state.turn = None;
        state
            .dialogue
            .message(&reply_id, "assistant", "Review complete", false);
        events.lock().unwrap().push(Event::WorkFinished {
            id: prompt.id,
            result: Ok(()),
            stopped: false,
        });
        assert!(engine.poll(&mut state).is_empty());
        let finished = to_gemini.try_recv().unwrap();
        assert_eq!(
            finished["toolResponse"]["functionResponses"][0]["response"],
            json!({"state":"finished", "reply":"Review complete"})
        );
    }

    #[test]
    fn interruption_keeps_work_pending_and_stopping_voice_keeps_acp_open() {
        let acp = Acp::default();
        let commands = acp.commands.clone();
        let engine = Adapter::new(Box::new(acp), Arc::default());
        let (transport, from_gemini, to_gemini) = Transport::fixture();
        *engine.state.lock().unwrap() = State {
            connected: true,
            voice: Some(session(transport)),
        };
        let mut state = Projection::default();
        from_gemini.send(call("work", "start_work")).unwrap();
        from_gemini.send(Update::Canceled("work".into())).unwrap();
        from_gemini.send(call("another", "start_work")).unwrap();
        engine.poll(&mut state);
        assert_eq!(commands.lock().unwrap().len(), 1);
        let refusal = to_gemini.try_recv().unwrap();
        assert_eq!(
            refusal["toolResponse"]["functionResponses"][0]["response"]["state"],
            "rejected"
        );
        engine.send(Command::StopVoice).unwrap();
        assert!(engine.state.lock().unwrap().connected);
        assert_eq!(commands.lock().unwrap().len(), 1);
        assert!(from_gemini.send(call("late", "work_status")).is_err());
    }

    #[test]
    fn voice_setup_failure_leaves_the_text_connection_usable() {
        let acp = Acp::default();
        let commands = acp.commands.clone();
        let engine = Adapter::new(Box::new(acp), Arc::default());
        let (transport, from_gemini, _) = Transport::fixture();
        *engine.state.lock().unwrap() = State {
            connected: true,
            voice: Some(session(transport)),
        };
        from_gemini
            .send(Update::Ended("Configure a Gemini API key".into()))
            .unwrap();
        let events = engine.poll(&mut Projection::default());
        assert!(matches!(
            &events[..],
            [Event::Voice { active: false, .. }, Event::VoiceError(_)]
        ));
        assert!(engine.state.lock().unwrap().connected);
        engine.send(Command::RefreshModels).unwrap();
        assert!(matches!(
            commands.lock().unwrap().as_slice(),
            [Command::RefreshModels]
        ));
    }
}
