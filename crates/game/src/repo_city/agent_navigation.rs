//! User-controlled navigation from chat references and MCP proposals.
use super::*;
use bevy::{
    input_focus::InputFocus,
    picking::events::PointerPress,
    text::{ComputedTextBlock, EditableText, TextBackgroundColor, TextLayoutInfo, Underline},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::Write,
    ops::Range,
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

const TTL: u64 = 60_000;
const MAX_PACKET: u64 = 8192;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    repository: PathBuf,
    revision: String,
    baseline: Option<String>,
    path: String,
    line: usize,
    #[serde(default)]
    end_line: Option<usize>,
    #[serde(default)]
    source_blob: Option<String>,
    before: bool,
}

impl Target {
    fn checked(repo: &Repository, path: &str, line: usize, before: bool) -> Result<Self, String> {
        let path = safe_path(path)?;
        let file = repo
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or("This file is not in the selected comparison.")?;
        let before = before || file.after.is_none();
        if (if before {
            file.before.as_ref()
        } else {
            file.after.as_ref()
        })
        .is_none()
        {
            return Err("This file is unavailable on the requested comparison side.".into());
        }
        if !(1..=1_000_000).contains(&line) {
            return Err("Supply a one-based source line between 1 and 1000000.".into());
        }
        Ok(Self {
            repository: repo.root.clone(),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            path,
            line,
            end_line: None,
            source_blob: (if before { &file.before } else { &file.after })
                .as_ref()
                .map(|blob| blob.id.clone()),
            before,
        })
    }
    fn current(&self, repo: &Repository) -> bool {
        self.repository == repo.root
            && self.revision == repo.revision
            && self.baseline == repo.baseline
            && Self::checked(repo, &self.path, self.line, self.before).is_ok_and(|current| {
                self.source_blob
                    .as_ref()
                    .is_none_or(|blob| current.source_blob.as_ref() == Some(blob))
                    && self.end_line.is_none_or(|end| {
                        checked_anchor(repo, &self.path, self.line, end, self.before).is_ok()
                    })
            })
    }
    fn destination(&self) -> review_notes::Destination {
        review_notes::Destination {
            revision: self.revision.clone(),
            anchor: review_notes::Anchor {
                path: self.path.clone(),
                before: self.before,
                start: self.line,
                end: self.end_line.unwrap_or(self.line),
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    version: u8,
    id: String,
    process: u32,
    created: u64,
    target: Target,
    reason: String,
}
impl Proposal {
    fn current(&self, repo: &Repository, process: u32, now: u64) -> bool {
        self.version == 1
            && self.process == process
            && now.checked_sub(self.created).is_some_and(|age| age <= TTL)
            && self.target.current(repo)
    }
}

fn safe_path(path: &str) -> Result<String, String> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains(['\0', '\n', '\r', ':'])
        || path.contains('\\')
    {
        return Err("Supply a repository-relative file path using forward slashes.".into());
    }
    if Path::new(path)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Navigation cannot leave this repository.".into());
    }
    Ok(path.into())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis().min(u64::MAX as u128) as u64)
}

fn packet_path(directory: &Path, process: u32) -> PathBuf {
    directory.join(format!("navigation-{process}.json"))
}

fn write_packet(directory: &Path, proposal: &Proposal) -> Result<(), String> {
    let bytes = serde_json::to_vec(proposal).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PACKET {
        return Err("Navigation proposal is too large.".into());
    }
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = packet_path(directory, proposal.process);
    let temporary = directory.join(format!(
        "navigation-{}.{}.pending",
        proposal.process, proposal.id
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.flush())
            .map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

/// Proposals are scoped to a fresh city process and never move its camera.
pub(super) fn propose(
    scope: &str,
    path: &str,
    line: usize,
    before: bool,
    reason: &str,
) -> Result<Value, String> {
    safe_path(path)?;
    if reason.chars().count() > 240 {
        return Err("Keep the navigation explanation within 240 characters.".into());
    }
    propose_range(scope, path, line, None, before, reason)
}

pub(super) fn displayed_repository(scope: &str) -> Result<(Repository, u32), String> {
    let focus = focus_context::read(scope);
    let snapshot = &focus["snapshot"];
    let process = snapshot["process"].as_u64().and_then(|value| u32::try_from(value).ok()).filter(|value| *value != 0).ok_or("No fresh city window is available for this checkout. Open the repository in Repo City first.")?;
    let revision = snapshot["comparison"]["revision"]
        .as_str()
        .ok_or("The city comparison is unavailable.")?;
    let baseline = snapshot["comparison"]["baseline"].as_str();
    let root = Path::new(scope);
    let repo = if snapshot["comparison"]["worktree"] == true {
        Repository::open_worktree(root)?
    } else {
        Repository::open_with_base(root, revision, baseline)?
    };
    if repo.revision != revision {
        return Err(
            "The checkout changed since the city published its focus. Retry after it refreshes."
                .into(),
        );
    }
    Ok((repo, process))
}

pub(super) fn checked_anchor(
    repo: &Repository,
    path: &str,
    line: usize,
    end: usize,
    before: bool,
) -> Result<review_notes::Anchor, String> {
    let target = Target::checked(repo, path, line, before)?;
    if end < line || end > 1_000_000 {
        return Err("The last source line must not precede the first.".into());
    }
    let index = repo
        .files
        .iter()
        .position(|file| file.path == target.path)
        .expect("validated path");
    let source = repo.source(index, target.before)?;
    if end > source.lines().count().max(1) {
        return Err("The requested source range is outside this file.".into());
    }
    Ok(review_notes::Anchor {
        path: target.path,
        before: target.before,
        start: line,
        end,
    })
}

pub(super) fn propose_range(
    scope: &str,
    path: &str,
    line: usize,
    end_line: Option<usize>,
    before: bool,
    reason: &str,
) -> Result<Value, String> {
    safe_path(path)?;
    if reason.chars().count() > 240 {
        return Err("Keep the navigation explanation within 240 characters.".into());
    }
    let (repo, process) = displayed_repository(scope)?;
    let mut target = Target::checked(&repo, path, line, before)?;
    if let Some(end) = end_line {
        checked_anchor(&repo, path, line, end, before)?;
        target.end_line = Some(end);
    }
    if line > 1 {
        let index = repo
            .files
            .iter()
            .position(|file| file.path == target.path)
            .expect("validated inventory path");
        let source = repo.source(index, target.before)?;
        if line > source.lines().count().max(1) {
            return Err("The requested source line is outside this file.".into());
        }
    }
    let created = now();
    let proposal = Proposal {
        version: 1,
        id: format!(
            "{}-{created}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| time.subsec_nanos())
        ),
        process,
        created,
        target,
        reason: reason.into(),
    };
    let directory = focus_context::directory(scope)
        .ok_or("Could not locate local city communication storage.")?;
    write_packet(&directory, &proposal)?;
    Ok(
        json!({"status":"awaiting_user","request_id":proposal.id,"path":proposal.target.path,"line":line,"end_line":target_end(&proposal.target),"expires_in_seconds":60,"message":"The city will show a proposal. It moves only after the user accepts with Y or Fly; N dismisses it."}),
    )
}

fn target_end(target: &Target) -> usize {
    target.end_line.unwrap_or(target.line)
}

#[derive(Clone, Copy, Component)]
enum Decision {
    Accept,
    Dismiss,
}

#[derive(Resource, Default)]
pub(super) struct Navigation {
    pending: Option<Proposal>,
    destination: Option<Target>,
    decision: Option<Decision>,
    next_poll: f64,
    followed: Option<(PathBuf, u64)>,
    next_follow: f64,
}
impl Navigation {
    fn follow_activity(
        &mut self,
        repo: &Repository,
        activity: &agent_engine::state::FileActivity,
        now: f64,
    ) -> bool {
        if now < self.next_follow
            || self
                .followed
                .as_ref()
                .is_some_and(|(root, serial)| root == &repo.root && *serial == activity.serial)
        {
            return false;
        }
        let Ok(target) = Target::checked(repo, &activity.path, activity.line, false) else {
            return false;
        };
        if !self.follow(&target, repo) {
            return false;
        }
        self.followed = Some((repo.root.clone(), activity.serial));
        self.next_follow = now + 1.5;
        true
    }
    pub(super) fn take_destination(
        &mut self,
        repo: &Repository,
    ) -> Option<review_notes::Destination> {
        self.destination
            .take()
            .filter(|target| target.current(repo))
            .map(|target| target.destination())
    }
    fn follow(&mut self, target: &Target, repo: &Repository) -> bool {
        if !target.current(repo) {
            return false;
        }
        self.pending = None;
        self.destination = Some(target.clone());
        true
    }
    fn decide(&mut self, accept: bool, repo: &Repository, process: u32, timestamp: u64) -> bool {
        let Some(proposal) = self.pending.take() else {
            return false;
        };
        accept && proposal.current(repo, process, timestamp) && self.follow(&proposal.target, repo)
    }
}
impl native_panel::Actions<Decision> for Navigation {
    fn activate(&mut self, action: Decision) {
        self.decision = Some(action);
    }
}

#[derive(Component)]
struct Toast;
#[derive(Component)]
struct ToastText;

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Navigation>()
            .add_systems(Startup, spawn_toast)
            .add_systems(
                Update,
                update
                    .before(city_navigation::navigate)
                    .before(listing::find::update),
            );
    }
}

fn spawn_toast(mut commands: Commands) {
    commands
        .spawn((
            Toast,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(76.),
                left: percent(23.),
                width: percent(54.),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(8.),
                padding: UiRect::all(px(12.)),
                border: UiRect::all(px(1.)),
                ..default()
            },
            GlobalZIndex(60),
            BackgroundColor(Color::srgb_u8(10, 29, 36)),
            BorderColor::all(Color::srgb_u8(77, 157, 151)),
        ))
        .with_children(|body| {
            body.spawn((
                ToastText,
                Text::new(""),
                native_panel::panel_font(),
                TextColor(native_panel::INK),
                Node {
                    max_height: px(64.),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            body.spawn(Node {
                column_gap: px(8.),
                ..default()
            })
            .with_children(|bar| {
                native_panel::button::<Decision, Navigation>(
                    bar,
                    "Y · Fly",
                    native_panel::INK,
                    Decision::Accept,
                );
                native_panel::button::<Decision, Navigation>(
                    bar,
                    "N · Dismiss",
                    native_panel::INK,
                    Decision::Dismiss,
                );
            });
        });
}

fn update(
    time: Res<Time>,
    repo: Res<Repo>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<(), Or<(With<EditableText>, With<source_editor::View>)>>,
    terminal: Res<terminal::Terminal>,
    search: Res<listing::find::Search>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut tablet: ResMut<review_tablet::Tablet>,
    mut navigation: ResMut<Navigation>,
    mut session: ResMut<agent_session::Session>,
    city: Res<city_navigation::Navigation>,
    mut toasts: Query<&mut Node, With<Toast>>,
    mut labels: Query<&mut Text, With<ToastText>>,
) {
    let timestamp = now();
    if navigation.pending.is_none() && time.elapsed_secs_f64() >= navigation.next_poll {
        navigation.next_poll = time.elapsed_secs_f64() + 0.3;
        if let Some(directory) = focus_context::directory(&repo.0.root.to_string_lossy()) {
            let incoming = packet_path(&directory, std::process::id());
            let path = directory.join(format!("navigation-{}.processing", std::process::id()));
            let claimed = std::fs::rename(&incoming, &path).is_ok();
            if claimed && let Ok(metadata) = std::fs::symlink_metadata(&path) {
                if metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && metadata.len() <= MAX_PACKET
                    && let Ok(bytes) = std::fs::read(&path)
                    && let Ok(proposal) = serde_json::from_slice::<Proposal>(&bytes)
                    && proposal.current(&repo.0, std::process::id(), timestamp)
                {
                    navigation.pending = Some(proposal);
                }
                let _ = std::fs::remove_file(path);
            }
        }
    }
    if navigation
        .pending
        .as_ref()
        .is_some_and(|proposal| !proposal.current(&repo.0, std::process::id(), timestamp))
    {
        navigation.pending = None;
    }
    let keyboard_allowed = windows.single().is_ok_and(|window| window.focused)
        && !terminal.active
        && !search.blocks_controls()
        && !focus.get().is_some_and(|entity| inputs.contains(entity))
        && !keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ]);
    if keyboard_allowed
        && keys.any_just_pressed([
            KeyCode::Escape,
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::Space,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
        ])
    {
        session.pause_following();
    }
    if keyboard_allowed && session.follows_agent() && !city.guides_camera() {
        if let Some(activity) = session.file_activity()
            && navigation.follow_activity(&repo.0, activity, time.elapsed_secs_f64())
        {
            tablet.request_close();
        }
    }
    if keyboard_allowed && navigation.pending.is_some() {
        if keys.just_pressed(KeyCode::KeyY) {
            navigation.decision = Some(Decision::Accept);
            keys.clear_just_pressed(KeyCode::KeyY);
        } else if keys.just_pressed(KeyCode::KeyN) {
            navigation.decision = Some(Decision::Dismiss);
            keys.clear_just_pressed(KeyCode::KeyN);
        }
    }
    if let Some(decision) = navigation.decision.take()
        && navigation.decide(
            matches!(decision, Decision::Accept),
            &repo.0,
            std::process::id(),
            timestamp,
        )
    {
        tablet.request_close();
    }
    for mut toast in &mut toasts {
        let display = if navigation.pending.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if toast.display != display {
            toast.display = display;
        }
    }
    if let Some(proposal) = &navigation.pending {
        let path = &proposal.target.path;
        let label = if path.chars().count() > 100 {
            format!(
                "…{}",
                path.chars()
                    .rev()
                    .take(99)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )
        } else {
            path.clone()
        };
        let text = format!(
            "Agent suggests {label}:{}{}{}",
            proposal.target.line,
            if proposal.reason.is_empty() { "" } else { "\n" },
            proposal.reason
        );
        for mut label in &mut labels {
            label.set_if_neq(Text::new(&text));
        }
    }
}

#[derive(Component, Clone)]
struct Reference(Target);
#[derive(Debug)]
struct Link {
    bytes: Range<usize>,
    text: String,
    target: Target,
}

fn resolve(repo: &Repository, raw: &str) -> Option<Target> {
    let raw = raw.trim();
    let (path, line) = if let Some((path, anchor)) = raw.split_once("#L") {
        (path, anchor.split('-').next()?.parse::<usize>().ok()?)
    } else {
        let mut parts = raw.rsplitn(3, ':');
        let last = parts.next()?;
        match (last.parse::<usize>(), parts.next()) {
            (Ok(line), Some(path)) => {
                if let Some(prefix) = parts.next() {
                    if let Ok(previous_line) = path.parse::<usize>() {
                        (prefix, previous_line)
                    } else {
                        return None;
                    }
                } else {
                    (path, line)
                }
            }
            _ => (raw, 1),
        }
    };
    let path = path.strip_prefix("file://").unwrap_or(path);
    let path = if Path::new(path).is_absolute() {
        Path::new(path).strip_prefix(&repo.root).ok()?.to_str()?
    } else {
        path.strip_prefix("./").unwrap_or(path)
    };
    if let Ok(target) = Target::checked(repo, path, line, false) {
        return Some(target);
    }
    let suffix = format!("/{path}");
    let mut matches = repo
        .files
        .iter()
        .filter(|file| file.path.ends_with(&suffix));
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Target::checked(repo, &first.path, line, false).ok()
}

fn links(text: &str, repo: &Repository) -> Vec<Link> {
    let mut links = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find('[') {
        let start = cursor + relative;
        let Some(middle) = text[start + 1..]
            .find("](")
            .map(|offset| start + 1 + offset)
        else {
            break;
        };
        let Some(end) = text[middle + 2..]
            .find(')')
            .map(|offset| middle + 2 + offset)
        else {
            break;
        };
        if let Some(target) = resolve(repo, &text[middle + 2..end]) {
            links.push(Link {
                bytes: start..end + 1,
                text: text[start + 1..middle].trim_matches('`').into(),
                target,
            });
        }
        cursor = end + 1;
    }
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find('`') {
        let start = cursor + relative;
        let Some(end) = text[start + 1..].find('`').map(|offset| start + 1 + offset) else {
            break;
        };
        if let Some(target) = resolve(repo, &text[start + 1..end]) {
            links.push(Link {
                bytes: start..end + 1,
                text: text[start + 1..end].into(),
                target,
            });
        }
        cursor = end + 1;
    }
    let boundary = |ch: char| ch.is_whitespace() || "`[](){}<>\"',;".contains(ch);
    let mut start = 0;
    for (end, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if boundary(ch) {
            if start < end {
                let token = &text[start..end];
                if token.contains(['/', '.', ':', '#'])
                    && let Some(target) =
                        resolve(repo, token).or_else(|| resolve(repo, token.trim_end_matches('.')))
                {
                    links.push(Link {
                        bytes: start..end,
                        text: token.into(),
                        target,
                    });
                }
            }
            start = end + ch.len_utf8();
        }
    }
    links.sort_by_key(|link| (link.bytes.start, std::cmp::Reverse(link.bytes.end)));
    let mut end = 0;
    links.retain(|link| {
        if link.bytes.start < end {
            false
        } else {
            end = link.bytes.end;
            true
        }
    });
    links.truncate(32);
    links
}

/// Real inline text spans retain wrapping; the text layout maps clicked runs to files.
pub(super) fn draw_links(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    repo: &Repository,
) -> Entity {
    let references = links(text, repo);
    let entity = parent
        .spawn((
            Text::new(""),
            native_panel::panel_font(),
            TextColor(Color::srgb_u8(218, 234, 236)),
            Node {
                width: percent(100.),
                ..default()
            },
        ))
        .with_children(|spans| {
            let mut cursor = 0;
            for link in references {
                if cursor < link.bytes.start {
                    spans.spawn((
                        TextSpan::new(&text[cursor..link.bytes.start]),
                        native_panel::panel_font(),
                        TextColor(Color::srgb_u8(218, 234, 236)),
                    ));
                }
                spans.spawn((
                    TextSpan::new(link.text),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(124, 217, 223)),
                    TextBackgroundColor(Color::srgb_u8(19, 51, 60)),
                    Underline,
                    Reference(link.target),
                ));
                cursor = link.bytes.end;
            }
            if cursor < text.len() {
                spans.spawn((
                    TextSpan::new(&text[cursor..]),
                    native_panel::panel_font(),
                    TextColor(Color::srgb_u8(218, 234, 236)),
                ));
            }
        })
        .id();
    parent.commands().entity(entity).observe(
        |event: On<PointerPress>,
         blocks: Query<(
            &ComputedTextBlock,
            &TextLayoutInfo,
            &UiGlobalTransform,
            &ComputedNode,
        )>,
         references: Query<&Reference>,
         repo: Res<Repo>,
         mut navigation: ResMut<Navigation>,
         mut tablet: ResMut<review_tablet::Tablet>| {
            if event.button != bevy::picking::pointer::PointerButton::Primary {
                return;
            }
            // Bevy's UI picker targets the inline span when its glyphs are hit.
            if let Ok(reference) = references.get(event.entity) {
                if navigation.follow(&reference.0, &repo.0) {
                    tablet.request_close();
                }
                return;
            }
            let Ok((block, layout, transform, node)) = blocks.get(event.entity) else {
                return;
            };
            let transform: bevy::math::Affine2 = transform.into();
            let local = transform.inverse().transform_point2(event.pointer.position)
                - node.content_box().min;
            for run in &layout.run_geometry {
                if run.bounds.contains(local)
                    && let Some(span) = block.entities().get(run.section_index as usize)
                    && let Ok(reference) = references.get(span.entity)
                    && navigation.follow(&reference.0, &repo.0)
                {
                    tablet.request_close();
                    break;
                }
            }
        },
    );
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repository() -> Repository {
        let mut repo = Repository {
            root: PathBuf::from("/checkout"),
            revision: "head".into(),
            baseline: None,
            files: Vec::new(),
            districts: Vec::new(),
        };
        repo.files = ["src/main.rs", "crates/explorer/build.rs", "other/build.rs"]
            .into_iter()
            .map(|path| repo_world::File {
                path: path.into(),
                before: None,
                after: Some(repo_world::Blob {
                    id: "blob".into(),
                    size: 10,
                    mode: "100644".into(),
                }),
                change: Change::Unchanged,
                position: [0.; 2],
            })
            .collect();
        repo
    }
    struct Checkout(PathBuf);

    impl Checkout {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let checkout = Self(std::env::temp_dir().join(format!(
                "repo-city-range-navigation-{}-{nonce}",
                std::process::id()
            )));
            std::fs::create_dir_all(checkout.0.join("src")).unwrap();
            let git = |args: &[&str]| {
                let output = std::process::Command::new("git")
                    .current_dir(&checkout.0)
                    .args([
                        "-c",
                        "commit.gpgsign=false",
                        "-c",
                        "core.hooksPath=/dev/null",
                    ])
                    .args(args)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            };
            git(&["init", "-q"]);
            std::fs::write(
                checkout.0.join("src/main.rs"),
                "first\nsecond\nthird\nfourth\n",
            )
            .unwrap();
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "source",
            ]);
            checkout
        }

        fn proposal(&self, repo: &Repository) -> Proposal {
            let mut target = Target::checked(repo, "src/main.rs", 2, false).unwrap();
            target.end_line = Some(3);
            assert!(checked_anchor(repo, &target.path, target.line, 3, false).is_ok());
            let proposal = Proposal {
                version: 1,
                id: "range-request".into(),
                process: 42,
                created: 1000,
                target,
                reason: "Explain these two lines".into(),
            };
            // Exercise the same packet representation that the MCP writes.
            serde_json::from_slice(&serde_json::to_vec(&proposal).unwrap()).unwrap()
        }
    }

    impl Drop for Checkout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_range_proposal_reveals_the_full_range_only_after_acceptance() {
        // Given a source range from a real live repository, carried through MCP's packet format.
        let checkout = Checkout::new();
        let repo = Repository::open_worktree(&checkout.0).unwrap();
        let proposal = checkout.proposal(&repo);
        let mut navigation = Navigation {
            pending: Some(proposal.clone()),
            ..default()
        };
        assert!(navigation.take_destination(&repo).is_none());
        // When N dismisses it, no destination is ever exposed.
        assert!(!navigation.decide(false, &repo, 42, 1001));
        assert!(navigation.pending.is_none());
        assert!(navigation.take_destination(&repo).is_none());
        // When Y accepts the unchanged source, both range ends survive.
        navigation.pending = Some(proposal);
        assert!(navigation.decide(true, &repo, 42, 1001));
        let destination = navigation.take_destination(&repo).unwrap();
        assert_eq!(destination.revision, repo.revision);
        assert_eq!(destination.anchor.path, "src/main.rs");
        assert_eq!((destination.anchor.start, destination.anchor.end), (2, 3));
        assert!(!destination.anchor.before);
        assert!(navigation.take_destination(&repo).is_none());
    }

    #[test]
    fn pending_and_accepted_ranges_reject_changed_worktree_bytes() {
        // Given a pending cue and a separately accepted destination on the same source bytes.
        let checkout = Checkout::new();
        let repo = Repository::open_worktree(&checkout.0).unwrap();
        let proposal = checkout.proposal(&repo);
        let mut pending = Navigation {
            pending: Some(proposal.clone()),
            ..default()
        };
        let mut accepted = Navigation {
            pending: Some(proposal.clone()),
            ..default()
        };
        assert!(accepted.decide(true, &repo, 42, 1001));
        // When content changes without changing HEAD, line count, or the proposed range.
        std::fs::write(
            checkout.0.join("src/main.rs"),
            "first\nnew-second\nthird\nfourth\n",
        )
        .unwrap();
        let changed = Repository::open_worktree(&checkout.0).unwrap();
        assert_eq!(repo.revision, changed.revision);
        assert_eq!(repo.baseline, changed.baseline);
        // Then Y cannot accept the stale cue; an unconsumed destination is rejected too.
        assert!(!pending.decide(true, &changed, 42, 1001));
        assert!(pending.take_destination(&changed).is_none());
        assert!(accepted.take_destination(&changed).is_none());
        assert!(pending.pending.is_none());
        // Malformed ranges must also be revalidated at acceptance, even with a matching blob.
        let mut invalid = proposal;
        invalid.target.end_line = Some(99);
        pending.pending = Some(invalid);
        assert!(!pending.decide(true, &repo, 42, 1001));
        assert!(pending.take_destination(&repo).is_none());
    }

    #[test]
    fn navigation_integrates_without_cycling_the_existing_control_schedule() {
        let mut app = App::new();
        app.add_plugins(Plugin);
        app.add_systems(
            Update,
            (
                listing::find::update,
                review_tablet::update,
                city_navigation::navigate,
            )
                .chain(),
        );
        let mut schedule = app
            .world_mut()
            .resource_mut::<bevy::ecs::schedule::Schedules>()
            .remove(Update)
            .unwrap();
        schedule
            .initialize(app.world_mut())
            .expect("agent navigation must fit the existing controls order");
    }

    #[test]
    fn following_coalesces_activity_and_waits_for_a_resolvable_file() {
        let repo = repository();
        let mut navigation = Navigation::default();
        let mut activity = agent_engine::state::FileActivity {
            serial: 1,
            path: "src/main.rs".into(),
            line: 7,
        };
        assert!(navigation.follow_activity(&repo, &activity, 0.));
        assert_eq!(navigation.take_destination(&repo).unwrap().anchor.start, 7);
        assert!(!navigation.follow_activity(&repo, &activity, 2.));
        activity.serial = 2;
        activity.line = 10;
        assert!(!navigation.follow_activity(&repo, &activity, 0.1));
        assert!(navigation.take_destination(&repo).is_none());
        assert!(navigation.follow_activity(&repo, &activity, 2.));
        assert_eq!(navigation.take_destination(&repo).unwrap().anchor.start, 10);
        activity.serial = 3;
        activity.path = "../other/private.rs".into();
        assert!(!navigation.follow_activity(&repo, &activity, 4.));
        assert!(navigation.take_destination(&repo).is_none());
    }

    #[test]
    fn proposed_navigation_waits_for_acceptance_and_dismissal_never_moves() {
        let repo = repository();
        let target = Target::checked(&repo, "src/main.rs", 7, false).unwrap();
        let proposal = Proposal {
            version: 1,
            id: "request".into(),
            process: 42,
            created: 1000,
            target,
            reason: "Follow this call".into(),
        };
        let mut navigation = Navigation {
            pending: Some(proposal.clone()),
            ..default()
        };
        assert!(navigation.take_destination(&repo).is_none());
        assert!(!navigation.decide(false, &repo, 42, 1001));
        assert!(navigation.take_destination(&repo).is_none());
        navigation.pending = Some(proposal.clone());
        assert!(navigation.decide(true, &repo, 42, 1001));
        assert_eq!(navigation.take_destination(&repo).unwrap().anchor.start, 7);
        navigation.pending = Some(proposal);
        assert!(!navigation.decide(true, &repo, 999, 1001));
        assert!(navigation.take_destination(&repo).is_none());
    }
    #[test]
    fn chat_links_are_inline_exact_safe_and_preserve_source_lines() {
        let repo = repository();
        let text = "See `crates/explorer/build.rs:12` and [main](/checkout/src/main.rs#L7). build.rs is ambiguous; /outside/src/main.rs and ../src/main.rs are forbidden.";
        let found = links(text, &repo);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "crates/explorer/build.rs:12");
        assert_eq!(found[0].target.line, 12);
        assert_eq!(found[1].text, "main");
        assert_eq!(found[1].target.line, 7);
        for path in [
            "../src/main.rs",
            "/src/main.rs",
            "src/../main.rs",
            "C:/src/main.rs",
            "src/main.rs\0",
        ] {
            assert!(Target::checked(&repo, path, 1, false).is_err());
        }
    }
    #[test]
    fn proposal_is_bound_to_process_snapshot_and_expiry() {
        let repo = repository();
        let proposal = Proposal {
            version: 1,
            id: "request".into(),
            process: 42,
            created: 1000,
            target: Target::checked(&repo, "src/main.rs", 1, false).unwrap(),
            reason: String::new(),
        };
        assert!(proposal.current(&repo, 42, 1001));
        assert!(!proposal.current(&repo, 42, 999));
        assert!(!proposal.current(&repo, 42, 1000 + TTL + 1));
        let mut changed = repo.clone();
        changed.revision.push('x');
        assert!(!proposal.current(&changed, 42, 1001));
    }

    #[test]
    fn an_accepted_destination_cannot_cross_into_another_checkout() {
        let repo = repository();
        let target = Target::checked(&repo, "src/main.rs", 1, false).unwrap();
        let mut navigation = Navigation::default();
        assert!(navigation.follow(&target, &repo));
        let mut other = repo.clone();
        other.root = PathBuf::from("/other-checkout");
        assert!(navigation.take_destination(&other).is_none());
    }
}
