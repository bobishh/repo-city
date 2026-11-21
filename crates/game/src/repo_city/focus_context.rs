//! A short-lived local projection of what the city camera can currently show.
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Mutex, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const FRESH_MILLIS: u64 = 6_000;
const WRITE_INTERVAL: f64 = 1.;
const MAX_LINES: usize = 100;
const MAX_LINE_CHARACTERS: usize = 400;

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Publisher>()
            .add_systems(Update, publish.after(listing::draw));
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Comparison {
    revision: String,
    baseline: Option<String>,
    worktree: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Line {
    #[serde(default)]
    before: bool,
    number: usize,
    text: String,
    truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Focus {
    path: String,
    before: bool,
    #[serde(default)]
    source_blob: Option<String>,
    face: Option<String>,
    visible_lines: Option<[usize; 2]>,
    readable_lines: Vec<Line>,
    text_truncated: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    version: u8,
    repository: String,
    observed_millis: u64,
    process: u32,
    active: bool,
    comparison: Comparison,
    aimed_path: Option<String>,
    selected_path: Option<String>,
    selected_before: bool,
    wall_face: Option<String>,
    focus: Option<Focus>,
    tablet_page: Option<String>,
    source_page: usize,
    camera_position: [f32; 3],
    camera_forward: [f32; 3],
}

impl Snapshot {
    fn current(&self, scope: &str, now: u64) -> bool {
        self.version == 1
            && self.repository == scope
            && now
                .checked_sub(self.observed_millis)
                .is_some_and(|age| age <= FRESH_MILLIS)
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis().min(u64::MAX as u128) as u64)
}

pub(super) fn directory(scope: &str) -> Option<PathBuf> {
    // Stable across app/MCP launches; the embedded repository is checked again on read.
    let hash = scope.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    });
    Some(
        repository_picker::settings_file()?
            .with_file_name("focus")
            .join(format!("{hash:016x}")),
    )
}

fn store(directory: &Path, snapshot: &Snapshot) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{}.json", snapshot.process));
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, snapshot).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn read_in(directory: &Path, scope: &str, now: u64) -> Value {
    let Some(entries) = std::fs::read_dir(directory).ok() else {
        return unavailable("No city focus has been published for this checkout.");
    };
    let mut latest: Option<Snapshot> = None;
    for entry in entries.flatten() {
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let Some(snapshot) = std::fs::read(entry.path())
            .ok()
            .and_then(|data| serde_json::from_slice::<Snapshot>(&data).ok())
        else {
            continue;
        };
        if snapshot.current(scope, now)
            && latest.as_ref().is_none_or(|latest| {
                (snapshot.active, snapshot.observed_millis)
                    > (latest.active, latest.observed_millis)
            })
        {
            latest = Some(snapshot);
        }
    }
    match latest {
        Some(snapshot) if snapshot.active => {
            json!({"live":true,"age_millis":now - snapshot.observed_millis,"snapshot":snapshot,"meaning":"Camera and readable viewport, not eye tracking."})
        }
        Some(snapshot) => {
            json!({"live":false,"age_millis":now - snapshot.observed_millis,"snapshot":snapshot,"reason":"The city window is inactive; this is its latest camera view."})
        }
        None => unavailable("No fresh city focus is available for this checkout."),
    }
}

fn unavailable(reason: &str) -> Value {
    json!({"live":false,"snapshot":null,"reason":reason})
}

pub(super) fn read(scope: &str) -> Value {
    directory(scope).map_or_else(
        || unavailable("Could not locate app storage."),
        |directory| read_in(&directory, scope, now_millis()),
    )
}

struct Writer {
    send: mpsc::SyncSender<(PathBuf, Snapshot)>,
    errors: Mutex<mpsc::Receiver<String>>,
}

impl Writer {
    fn new() -> Result<Self, String> {
        let (send, receive) = mpsc::sync_channel::<(PathBuf, Snapshot)>(1);
        let (errors, failures) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("city-focus".into())
            .spawn(move || {
                while let Ok((directory, snapshot)) = receive.recv() {
                    if let Err(error) = store(&directory, &snapshot) {
                        let _ = errors.try_send(error);
                    }
                }
            })
            .map_err(|error| format!("Could not start city focus writer: {error}"))?;
        Ok(Self {
            send,
            errors: Mutex::new(failures),
        })
    }
}

#[derive(Resource)]
pub(super) struct Publisher {
    writer: Option<Writer>,
    next: f64,
    error_reported: bool,
}

impl Default for Publisher {
    fn default() -> Self {
        let writer = match Writer::new() {
            Ok(writer) => Some(writer),
            Err(error) => {
                warn!("{error}");
                None
            }
        };
        Self {
            writer,
            next: 0.,
            error_reported: false,
        }
    }
}

fn publish(
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    nearest: Res<Nearest>,
    listing: Res<listing::Listing>,
    review: Res<review_notes::Review>,
    tablet: Res<review_tablet::Tablet>,
    filter: Res<file_facade::ChangesOnly>,
    camera: Query<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut publisher: ResMut<Publisher>,
) {
    if !publisher.due(time.elapsed_secs_f64()) {
        return;
    }
    let (Ok(camera), Ok(window)) = (camera.single(), windows.single()) else {
        return;
    };
    let visible = repo
        .0
        .files
        .get(explorer.selected)
        .filter(|file| !review.blocked && !review.tablet_open && filter.includes(file))
        .and_then(|file| {
            listing.visible_wall_lines(
                file,
                camera,
                &explorer,
                window.width() / window.height().max(1.),
            )
        });
    let aimed_path = nearest
        .file
        .and_then(|aim| repo.0.files.get(aim.index))
        .filter(|file| filter.includes(file))
        .map(|file| file.path.as_str());
    publisher.submit(
        &repo.0,
        &explorer,
        camera,
        View {
            active: window.focused,
            aimed_path,
            wall_face: listing.visible_face(),
            visible,
            tablet_page: tablet
                .page_name()
                .or_else(|| review.tablet_open.then_some("review_notes")),
            source_page: explorer.page + 1,
        },
    );
}

pub(super) struct View<'a> {
    pub active: bool,
    pub aimed_path: Option<&'a str>,
    pub wall_face: Option<listing::Face>,
    pub visible: Option<(listing::Face, Vec<listing::VisibleLine<'a>>)>,
    pub tablet_page: Option<&'a str>,
    pub source_page: usize,
}

fn face_name(face: listing::Face) -> &'static str {
    match face {
        listing::Face::Source => "source",
        listing::Face::Diff => "diff",
        listing::Face::Blame => "blame",
    }
}

impl Publisher {
    pub(super) fn due(&mut self, now: f64) -> bool {
        let error = self
            .writer
            .as_ref()
            .and_then(|writer| writer.errors.try_lock().ok()?.try_recv().ok());
        if let Some(error) = error
            && !self.error_reported
        {
            warn!("Could not publish city focus: {error}");
            self.error_reported = true;
        }
        if self.writer.is_none() || now < self.next {
            return false;
        }
        self.next = now + WRITE_INTERVAL;
        true
    }

    pub(super) fn submit(
        &mut self,
        repo: &Repository,
        explorer: &Explorer,
        camera: &Transform,
        view: View<'_>,
    ) {
        let scope = repo.root.to_string_lossy().into_owned();
        let Some(directory) = directory(&scope) else {
            return;
        };
        let selected = repo.files.get(explorer.selected);
        let focus = selected.zip(view.visible).map(|(file, (face, lines))| {
            let numbers = lines
                .iter()
                .filter(|line| line.before == explorer.before)
                .map(|line| line.line.number);
            let visible_lines = numbers.fold(None::<[usize; 2]>, |range, number| {
                Some(match range {
                    None => [number, number],
                    Some([a, b]) => [a.min(number), b.max(number)],
                })
            });
            let mut grouped: Vec<(bool, usize, String)> = Vec::new();
            for fragment in lines.into_iter().filter(|line| line.readable) {
                let before = fragment.before;
                let number = fragment.line.number;
                let text = fragment.line.text.as_str();
                if let Some((last_before, last_number, last_text)) = grouped.last_mut()
                    && *last_before == before
                    && *last_number == number
                {
                    last_text.push_str(text);
                } else {
                    grouped.push((before, number, text.to_owned()));
                }
            }
            let omitted = grouped.len() > MAX_LINES;
            let readable_lines = grouped
                .into_iter()
                .take(MAX_LINES)
                .map(|(before, number, text)| {
                    let original =
                        explorer
                            .comparison
                            .as_ref()
                            .map_or(&explorer.highlighted, |diff| {
                                if before { &diff.before } else { &diff.after }
                            });
                    let whole_line = original
                        .get(number.saturating_sub(1))
                        .map(|line| line.text.as_str());
                    Line {
                        before,
                        number,
                        truncated: whole_line != Some(text.as_str())
                            || text.chars().count() > MAX_LINE_CHARACTERS,
                        text: text.chars().take(MAX_LINE_CHARACTERS).collect(),
                    }
                })
                .collect::<Vec<_>>();
            let text_truncated = omitted || readable_lines.iter().any(|line| line.truncated);
            Focus {
                path: file.path.clone(),
                before: explorer.before,
                source_blob: (if explorer.before {
                    &file.before
                } else {
                    &file.after
                })
                .as_ref()
                .map(|blob| blob.id.clone()),
                face: Some(face_name(face).into()),
                visible_lines,
                readable_lines,
                text_truncated,
            }
        });
        let snapshot = Snapshot {
            version: 1,
            repository: scope,
            observed_millis: now_millis(),
            process: std::process::id(),
            active: view.active,
            comparison: Comparison {
                revision: repo.revision.clone(),
                baseline: repo.baseline.clone(),
                worktree: repo.is_worktree(),
            },
            aimed_path: view.aimed_path.map(str::to_owned),
            selected_path: selected.map(|file| file.path.clone()),
            selected_before: explorer.before,
            wall_face: view.wall_face.map(face_name).map(str::to_owned),
            focus,
            tablet_page: view.tablet_page.map(str::to_owned),
            source_page: view.source_page,
            camera_position: camera.translation.to_array(),
            camera_forward: camera.forward().to_array(),
        };
        if let Some(writer) = &self.writer {
            match writer.send.try_send((directory, snapshot)) {
                Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    if !self.error_reported {
                        warn!("City focus writer stopped.");
                        self.error_reported = true;
                    }
                    self.writer = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(scope: &str, at: u64) -> Snapshot {
        Snapshot {
            version: 1,
            repository: scope.into(),
            observed_millis: at,
            process: 1,
            active: true,
            comparison: Comparison {
                revision: "worktree:a".into(),
                baseline: Some("a".into()),
                worktree: true,
            },
            aimed_path: Some("src/a.rs".into()),
            selected_path: Some("src/a.rs".into()),
            selected_before: false,
            wall_face: Some("source".into()),
            focus: Some(Focus {
                path: "src/a.rs".into(),
                before: false,
                source_blob: None,
                face: Some("source".into()),
                visible_lines: Some([4, 8]),
                readable_lines: vec![Line {
                    before: false,
                    number: 4,
                    text: "let x = 1;".into(),
                    truncated: false,
                }],
                text_truncated: false,
            }),
            tablet_page: None,
            source_page: 1,
            camera_position: [0., 1., 2.],
            camera_forward: [0., 0., -1.],
        }
    }
    #[test]
    fn focus_is_scoped_expires_and_rejects_future_or_unknown_versions() {
        let mut value = snapshot("/checkout/a", 100_000);
        assert!(value.current("/checkout/a", 106_000));
        assert!(!value.current("/checkout/a", 106_001));
        assert!(!value.current("/checkout/b", 100_001));
        assert!(!value.current("/checkout/a", 99_999));
        value.version = 2;
        assert!(!value.current("/checkout/a", 100_001));
    }
    #[test]
    fn stored_focus_prefers_active_windows_and_never_reuses_stale_or_other_checkout_data() {
        let directory = std::env::temp_dir().join(format!(
            "repo-city-focus-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let mut value = snapshot("/checkout/a", 100_000);
        store(&directory, &value).unwrap();
        assert_eq!(read_in(&directory, "/checkout/a", 100_100)["live"], true);
        assert_eq!(read_in(&directory, "/checkout/a", 106_001)["live"], false);
        assert_eq!(read_in(&directory, "/checkout/b", 100_100)["live"], false);
        value.process = 2;
        value.active = false;
        value.observed_millis = 100_050;
        store(&directory, &value).unwrap();
        assert_eq!(
            read_in(&directory, "/checkout/a", 100_100)["snapshot"]["process"],
            1
        );
        value.process = 1;
        store(&directory, &value).unwrap();
        assert_eq!(read_in(&directory, "/checkout/a", 100_100)["live"], false);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn focus_roundtrip_keeps_comparison_side_and_bounded_readable_text() {
        let value = snapshot("/checkout/a", 100_000);
        let restored: Snapshot =
            serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(restored.focus.as_ref().unwrap().visible_lines, Some([4, 8]));
        assert_eq!(restored.focus.as_ref().unwrap().readable_lines[0].number, 4);
        assert!(!restored.focus.unwrap().before);
        assert_eq!(restored.comparison.baseline.as_deref(), Some("a"));
    }
}
