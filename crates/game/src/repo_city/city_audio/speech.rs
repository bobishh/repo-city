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
                // Strip speech markup and punctuation; code remains data for the local voice.
                let word = word
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '_' {
                            c
                        } else {
                            ' '
                        }
                    })
                    .collect::<String>();
                let word = word.trim().replace('_', " ");
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

#[cfg(target_os = "macos")]
fn speak(text: &str, volume: f32) -> Result<Child, String> {
    let mut child = Command::new("/usr/bin/say")
        .args(["-v", "Zarvox", "-r", "238"])
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
fn speak(_: &str, _: f32) -> Result<Child, String> {
    Err("Robot voice is available on macOS.".into())
}

fn run(shared: Shared) {
    let mut child = None;
    let mut revision = 0;
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
        if let Some((text, volume)) = next {
            stop(&mut child);
            if let Some(text) = text {
                match speak(&text, volume) {
                    Ok(spoken) => child = Some(spoken),
                    Err(error) => {
                        if let Ok(mut request) = lock.lock() {
                            request.error = Some(error);
                        }
                    }
                }
            }
        }
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
    }
    stop(&mut child);
}

#[cfg(test)]
mod tests {
    use super::*;

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
