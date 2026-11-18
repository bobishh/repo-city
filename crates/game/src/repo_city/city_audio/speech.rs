//! Short quotations follow an estimated reading position, never an inferred gaze.
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

pub(super) fn visible_text(text: &str) -> &str {
    let mut end = text.len().min(512);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn spoken_identifier(text: &str) -> String {
    let mut spoken = String::new();
    let mut characters = text.chars().peekable();
    let mut previous: Option<char> = None;
    while let Some(character) = characters.next() {
        if character.is_alphanumeric() {
            if character.is_uppercase()
                && previous.is_some_and(|before| {
                    before.is_lowercase()
                        || (before.is_uppercase()
                            && characters.peek().is_some_and(|next| next.is_lowercase()))
                })
            {
                spoken.push(' ');
            }
            spoken.push(character);
            previous = Some(character);
        } else {
            if !spoken.ends_with(' ') {
                spoken.push(' ');
            }
            previous = None;
        }
    }
    spoken.trim().to_owned()
}

#[derive(Default)]
pub(super) struct Reading {
    words: Vec<(usize, String)>,
    characters: usize,
    seconds: f32,
    elapsed: f32,
    next: f32,
    random: u32,
}

impl Reading {
    pub fn start(text: &str, seconds: f32, seed: u32) -> Self {
        let mut characters = 0;
        let words = text
            .split_whitespace()
            .filter_map(|word| {
                let position = characters;
                characters += word.chars().count();
                let word = spoken_identifier(word);
                (!word.is_empty()).then_some((position, word))
            })
            .collect();
        let mut reading = Self {
            words,
            characters,
            seconds,
            random: seed.max(1),
            ..Self::default()
        };
        reading.next = (seconds * 0.3).clamp(1.5, 4.5);
        reading
    }

    pub fn tick(&mut self, delta: f32) -> Option<String> {
        self.elapsed += delta.max(0.);
        if self.elapsed < self.next
            || self.seconds <= 0.
            || self.elapsed > self.seconds
            || self.words.is_empty()
        {
            return None;
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 17;
        self.random ^= self.random << 5;
        self.next = self.elapsed + 9. + (self.random % 9) as f32;
        let position = (self.elapsed / self.seconds * self.characters as f32) as usize;
        let first = self
            .words
            .partition_point(|(offset, _)| *offset < position)
            .min(self.words.len() - 1);
        let count = 3 + self.random as usize % 4;
        let text = self.words[first..(first + count).min(self.words.len())]
            .iter()
            .map(|(_, word)| word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Some(text)
    }
}

/// One replaceable request; there is no growing narration queue when focus changes.
#[derive(Default)]
struct Request {
    revision: u64,
    text: Option<String>,
    volume: f32,
    shutdown: bool,
    error: Option<String>,
}

type Shared = Arc<(Mutex<Request>, Condvar)>;

pub(super) struct Voice(Shared, Option<thread::JoinHandle<()>>);

impl Voice {
    pub fn new() -> Self {
        let shared = Shared::default();
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("city-voice".into())
            .spawn(move || run(worker));
        match thread {
            Ok(thread) => Self(shared, Some(thread)),
            Err(error) => {
                if let Ok(mut request) = shared.0.lock() {
                    request.error = Some(format!("Could not start robot voice: {error}"));
                }
                Self(shared, None)
            }
        }
    }

    pub fn speak(&self, text: String, volume: f32) {
        self.request(Some(text), volume);
    }
    pub fn stop(&self) {
        self.request(None, 0.);
    }

    fn request(&self, text: Option<String>, volume: f32) {
        let (lock, wake) = &*self.0;
        // The worker never holds this lock while starting, waiting for, or stopping speech.
        if let Ok(mut request) = lock.lock() {
            request.revision = request.revision.wrapping_add(1);
            request.text = text;
            request.volume = volume.clamp(0., 1.);
            wake.notify_one();
        }
    }

    pub fn error(&self) -> Option<String> {
        self.0.0.lock().ok()?.error.take()
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        let (lock, wake) = &*self.0;
        if let Ok(mut request) = lock.lock() {
            request.shutdown = true;
            wake.notify_one();
        }
        if let Some(thread) = self.1.take() {
            let _ = thread.join();
        }
    }
}

fn stop(child: &mut Option<Child>) {
    if let Some(mut child) = child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn apply_request(
    child: &mut Option<Child>,
    text: Option<&str>,
    start: impl FnOnce(&str) -> Result<Child, String>,
) -> Result<(), String> {
    match text {
        None => stop(child),
        Some(text) if child.is_none() => *child = Some(start(text)?),
        Some(_) => {} // Keep the current phrase; never queue or replace it.
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn system_voice() -> Option<String> {
    let voices = Command::new("/usr/bin/say")
        .args(["-v", "?"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&voices.stdout)
        .lines()
        .any(|line| line.split_whitespace().next() == Some("Samantha"))
        .then(|| "Samantha".to_owned())
}

#[cfg(not(target_os = "macos"))]
fn system_voice() -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn speak(text: &str, volume: f32, voice: Option<&str>) -> Result<Child, String> {
    let mut command = Command::new("/usr/bin/say");
    if let Some(voice) = voice {
        command.args(["-v", voice]);
    }
    let mut child = command
        .args(["-r", "175"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Robot voice unavailable: {error}"))?;
    let result = child
        .stdin
        .take()
        .ok_or_else(|| "Could not open robot voice input.".to_owned())
        .and_then(|mut input| {
            write!(input, "[[volm {volume:.2}]] {text}").map_err(|error| error.to_string())
        });
    if let Err(error) = result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child)
}

#[cfg(not(target_os = "macos"))]
fn speak(_: &str, _: f32, _: Option<&str>) -> Result<Child, String> {
    Err("Robot voice is available on macOS.".into())
}

fn run(shared: Shared) {
    let mut child: Option<Child> = None;
    let mut revision = 0;
    let mut voice: Option<Option<String>> = None;
    loop {
        let (lock, wake) = &*shared;
        let Ok(request) = lock.lock() else { break };
        let unchanged = |request: &mut Request| request.revision == revision && !request.shutdown;
        let waiting = if child.is_some() {
            wake.wait_timeout_while(request, Duration::from_millis(80), unchanged)
                .map(|(request, _)| request)
                .ok()
        } else {
            wake.wait_while(request, unchanged).ok()
        };
        let Some(mut request) = waiting else {
            break;
        };
        if request.shutdown {
            drop(request);
            break;
        }
        let next = if request.revision != revision {
            revision = request.revision;
            Some((request.text.take(), request.volume))
        } else {
            None
        };
        drop(request);
        if let Some(status) = child
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten())
        {
            child = None;
            if !status.success()
                && let Ok(mut request) = lock.lock()
            {
                request.error = Some("The system robot voice could not speak this excerpt.".into());
            }
        }
        if let Some((text, volume)) = next
            && let Err(error) = apply_request(&mut child, text.as_deref(), |text| {
                speak(
                    text,
                    volume,
                    voice.get_or_insert_with(system_voice).as_deref(),
                )
            })
            && let Ok(mut request) = lock.lock()
        {
            request.error = Some(error);
        }
    }
    stop(&mut child);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_new_quote_keeps_the_current_phrase_alive_but_explicit_stop_cancels_it() {
        let start = |_: &str| {
            Command::new("/bin/sleep")
                .arg("2")
                .spawn()
                .map_err(|error| error.to_string())
        };
        let mut child = Some(start("first").unwrap());
        let original = child.as_ref().unwrap().id();
        apply_request(&mut child, Some("new focus"), start).unwrap();
        let continued = child.as_ref().unwrap().id() == original;
        let alive = child.as_mut().unwrap().try_wait().unwrap().is_none();
        apply_request(&mut child, None, |_| {
            panic!("stop must not create a new utterance")
        })
        .unwrap();
        assert!(child.is_none());
        assert!(
            continued && alive,
            "Changing focus must leave the same speaking process alive."
        );
    }

    #[test]
    fn reading_quotations_advance_and_stop_at_the_visible_block_end() {
        let text = (0..100)
            .map(|word| format!("w{word:03}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut first = Reading::start(&text, 100., 19);
        let mut second = Reading::start(&text, 100., 19);
        let excerpt = first.tick(10.).unwrap();
        assert_eq!(Some(excerpt.clone()), second.tick(10.));
        assert!(excerpt.starts_with("w010"), "{excerpt}");
        let later = first.tick(20.).unwrap();
        assert!(later.starts_with("w030"), "{later}");
        assert_eq!(first.tick(100.), None);
        // A new focus restarts its own position and does not retain the old text.
        let mut next = Reading::start("new words for another file", 12., 1);
        assert!(!next.tick(4.).unwrap().contains("w030"));
    }

    #[test]
    fn code_identifiers_are_spoken_as_separate_words() {
        let mut reading = Reading::start("veryLongFunctionName(good_input)", 20., 3);
        assert_eq!(
            reading.tick(6.).as_deref(),
            Some("very Long Function Name good input")
        );
    }

    #[test]
    fn unicode_code_is_quoted_without_slicing_inside_characters() {
        let long = "日本語 🦀 ".repeat(100);
        let clipped = visible_text(&long);
        assert!(clipped.len() <= 512 && long.starts_with(clipped));
        let mut reading =
            Reading::start("привет_мир café λογος 日本語 🦀 результат_чтения", 20., 3);
        let excerpt = reading.tick(6.).unwrap();
        assert!(excerpt.contains("λογος"));
        assert!(excerpt.contains("日本語"));
        assert!(excerpt.chars().all(|c| c.is_alphanumeric() || c == ' '));
    }

    #[test]
    fn code_cannot_inject_voice_markup_or_command_arguments() {
        let mut reading = Reading::start("--voice evil [[volm 99]] $(anything) code_name", 20., 3);
        let excerpt = reading.tick(6.).unwrap();
        assert!(excerpt.chars().all(|c| c.is_alphanumeric() || c == ' '));
    }
}
