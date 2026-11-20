//! Two city presentation surfaces for the active conversation and sketch.
use super::*;
use review_party::BoardContent;
mod document;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 608;
const INK: [f32; 3] = [0.63, 0.91, 0.85];
const BODY: [f32; 3] = [0.84, 0.89, 0.91];

#[derive(Default)]
struct Boards {
    panels: Option<[Entity; 2]>,
    textures: Option<[Handle<Image>; 2]>,
    previous: Option<(bool, BoardContent)>,
    next_update: f64,
    readme: document::Readme,
    waiting_for_asset: bool,
    #[cfg(not(target_arch = "wasm32"))]
    local_scope: Option<PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    draft: Option<sketch::Sketch>,
    #[cfg(not(target_arch = "wasm32"))]
    next_draft_check: f64,
}

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update.after(review_party::ProjectRoom));
    }
}

fn update(
    mut commands: Commands,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    time: Res<Time>,
    party: Res<review_party::Party>,
    #[cfg(not(target_arch = "wasm32"))] agent: Res<agent_session::Session>,
    mut state: Local<Boards>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(district) = repo.0.districts.first() else {
        if let Some(panels) = state.panels.take() {
            for panel in panels {
                commands.entity(panel).despawn();
            }
            *state = default();
        }
        return;
    };
    if repo.is_changed() || state.panels.is_none() {
        state.readme.select(&repo.0);
        state.previous = None;
        let front = repo
            .0
            .districts
            .iter()
            .map(|district| district.position[1])
            .fold(f32::INFINITY, f32::min);
        let center = district.position[0] + district.size[0] / 2.;
        let transforms =
            [-12., 12.].map(|offset| Transform::from_xyz(center + offset, 7., front - 10.));
        if let Some(panels) = state.panels {
            for (panel, transform) in panels.into_iter().zip(transforms) {
                commands.entity(panel).insert(transform);
            }
        } else {
            let mesh = meshes.add(Rectangle::new(22., 13.));
            let textures = [
                images.add(canvas(&set, "Chat").image()),
                images.add(canvas(&set, "Documents").image()),
            ];
            state.panels = Some(std::array::from_fn(|index| {
                commands
                    .spawn((
                        Name::new(if index == 0 {
                            "City chat board"
                        } else {
                            "City document board"
                        }),
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color_texture: Some(textures[index].clone()),
                            unlit: true,
                            ..default()
                        })),
                        transforms[index],
                        bevy::light::NotShadowCaster,
                    ))
                    .id()
            }));
            state.textures = Some(textures);
        }
    }
    let now = time.elapsed_secs_f64();
    if now < state.next_update {
        return;
    }
    state.next_update = now + 0.25;

    let room = party.board_content();
    let shared = room.is_some();
    #[cfg(not(target_arch = "wasm32"))]
    let content = room.unwrap_or_else(|| {
        if now >= state.next_draft_check || state.local_scope.as_ref() != Some(&repo.0.root) {
            state.next_draft_check = now + 1.;
            state.local_scope = Some(repo.0.root.clone());
            state.draft = repo.0.root.to_str().and_then(sketch::staged);
        }
        BoardContent {
            chat: agent.chat_board(),
            sketch: state.draft.clone(),
        }
    });
    #[cfg(target_arch = "wasm32")]
    let content = room.unwrap_or_default();

    let current_document = content.sketch.clone().or_else(|| state.readme.get(&repo.0));
    if let Some(textures) = state.textures.clone() {
        let redraw_chat = state
            .previous
            .as_ref()
            .is_none_or(|(was_shared, previous)| {
                shared != *was_shared || content.chat != previous.chat
            });
        let redraw_document = state.waiting_for_asset
            || state
                .previous
                .as_ref()
                .is_none_or(|(was_shared, previous)| {
                    shared != *was_shared || current_document != previous.sketch
                });
        if redraw_chat && let Some(mut image) = images.get_mut(&textures[0]) {
            *image = render_chat(&set, shared, &content);
        }
        if redraw_document && let Some(mut image) = images.get_mut(&textures[1]) {
            let (document, waiting) = document::render(&set, &repo.0, current_document.as_ref());
            *image = document;
            state.waiting_for_asset = waiting;
        }
        state.previous = Some((
            shared,
            BoardContent {
                sketch: current_document,
                ..content
            },
        ));
    }
}

fn canvas(set: &WorldSet, title: &str) -> code_texture::Canvas {
    let tint = set
        .background
        .map(|value| (value.clamp(0., 1.) * 255.) as u8);
    let mut canvas = code_texture::Canvas::new(WIDTH, HEIGHT);
    canvas.rect(0, 0, WIDTH, HEIGHT, [25, 55, 63]);
    canvas.rect(4, 4, WIDTH - 8, HEIGHT - 8, tint);
    canvas.rect(4, 4, WIDTH - 8, 88, [13, 31, 39]);
    canvas.text(title, 34., 64., 43., |_| INK);
    canvas
}

fn render_chat(set: &WorldSet, shared: bool, content: &BoardContent) -> Image {
    let mut canvas = canvas(set, if shared { "Room chat" } else { "Chat" });
    for (index, entry) in content.chat.iter().rev().take(5).enumerate() {
        let top = 103. + index as f32 * 96.;
        canvas.text(&entry.login, 34., top + 26., 25., |_| INK);
        let body = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut chars = body.chars();
        let first = chars.by_ref().take(73).collect::<String>();
        let mut second = chars.by_ref().take(73).collect::<String>();
        if chars.next().is_some() {
            second.push('…');
        }
        canvas.text(&first, 34., top + 54., 22., |_| BODY);
        canvas.text(&second, 34., top + 80., 22., |_| BODY);
    }
    if content.chat.is_empty() {
        let message = if shared {
            "Messages from this room appear here."
        } else {
            "Open Chat on VibePad."
        };
        canvas.text(message, 36., 148., 27., |_| BODY);
    }
    canvas.image()
}
