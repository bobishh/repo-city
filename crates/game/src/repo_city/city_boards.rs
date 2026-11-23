//! Two city presentation surfaces for the active conversation and sketch.
use super::*;
use review_party::BoardContent;
mod document;
mod input;
mod page;
use page::Page;

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 608;
const INK: [f32; 3] = [0.63, 0.91, 0.85];
const BODY: [f32; 3] = [0.84, 0.89, 0.91];

#[derive(Resource)]
pub(super) struct Placement {
    pub transforms: [Transform; 2],
    pub size: Vec2,
}

#[derive(Default)]
struct Boards {
    panels: Option<[Entity; 2]>,
    textures: Option<[Handle<Image>; 2]>,
    previous: Option<(bool, BoardContent)>,
    pages: [Option<Page>; 2],
    offsets: [f32; 2],
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

#[derive(Component)]
struct Board {
    index: usize,
    size: Vec2,
    scrollable: bool,
}

#[derive(Resource, Default)]
struct Scroll([f32; 2]);

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ScrollInput;

pub(super) struct Plugin;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scroll>()
            .add_systems(
                Update,
                input::route
                    .in_set(ScrollInput)
                    .before(super::flight)
                    .before(city_navigation::navigate)
                    .before(listing::controls)
                    .before(super::interaction),
            )
            .add_systems(
                Update,
                update.after(review_party::ProjectRoom).after(ScrollInput),
            );
    }
}

fn update(
    mut commands: Commands,
    repo: Res<Repo>,
    set: Res<WorldSet>,
    time: Res<Time>,
    party: Res<review_party::Party>,
    placement: Option<Res<Placement>>,
    #[cfg(not(target_arch = "wasm32"))] agent: Res<agent_session::Session>,
    mut state: Local<Boards>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scroll: ResMut<Scroll>,
) {
    let delta = std::mem::take(&mut scroll.0);
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
        let front = repo
            .0
            .districts
            .iter()
            .map(|district| district.position[1])
            .fold(f32::INFINITY, f32::min);
        let center = district.position[0] + district.size[0] / 2.;
        let (transforms, size) = placement.as_ref().map_or_else(
            || {
                (
                    [-12., 12.].map(|offset| Transform::from_xyz(center + offset, 7., front - 10.)),
                    Vec2::new(22., 13.),
                )
            },
            |placement| (placement.transforms, placement.size),
        );
        if let Some(panels) = state.panels {
            for (panel, transform) in panels.into_iter().zip(transforms) {
                commands.entity(panel).insert(transform);
            }
        } else {
            let mesh = meshes.add(Rectangle::from_size(size));
            let textures = [
                images.add(canvas(&set, "Chat").image()),
                images.add(canvas(&set, "Documents").image()),
            ];
            state.panels = Some(std::array::from_fn(|index| {
                commands
                    .spawn((
                        Name::new(if index == 0 {
                            "Chat board"
                        } else {
                            "Document board"
                        }),
                        Mesh3d(mesh.clone()),
                        Board {
                            index,
                            size,
                            scrollable: false,
                        },
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
    let mut redraw = [false; 2];
    if now >= state.next_update {
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
        let new_scope = state
            .previous
            .as_ref()
            .is_none_or(|(was_shared, _)| *was_shared != shared);
        redraw[0] = new_scope
            || state
                .previous
                .as_ref()
                .is_some_and(|(_, previous)| content.chat != previous.chat);
        if redraw[0] {
            let follow = new_scope
                || state.pages[0]
                    .as_ref()
                    .is_none_or(|page| state.offsets[0] >= page.scroll_limit() - 1.);
            let page = layout_chat(&set, shared, &content);
            if follow {
                state.offsets[0] = page.scroll_limit();
            }
            state.pages[0] = Some(page);
        }
        let new_document = new_scope
            || state
                .previous
                .as_ref()
                .is_some_and(|(_, previous)| current_document != previous.sketch);
        redraw[1] = new_document || state.waiting_for_asset;
        if redraw[1] {
            let (page, waiting) = document::layout(&repo.0, current_document.as_ref());
            state.pages[1] = Some(page);
            state.waiting_for_asset = waiting;
            if new_document {
                state.offsets[1] = 0.;
            }
        }
        state.previous = Some((
            shared,
            BoardContent {
                sketch: current_document,
                ..content
            },
        ));
    }
    if let (Some(panels), Some(textures)) = (state.panels, state.textures.clone()) {
        for index in 0..2 {
            let Some(page) = &state.pages[index] else {
                continue;
            };
            let limit = page.scroll_limit();
            let offset = (state.offsets[index] + delta[index]).clamp(0., limit);
            if (redraw[index] || offset != state.offsets[index])
                && let Some(mut image) = images.get_mut(&textures[index])
            {
                *image = page.image(&set, offset);
            }
            if redraw[index] {
                commands
                    .entity(panels[index])
                    .entry::<Board>()
                    .and_modify(move |mut board| board.scrollable = limit > 0.);
            }
            state.offsets[index] = offset;
        }
    }
}

fn background(set: &WorldSet) -> [u8; 3] {
    set.background
        .map(|value| (value.clamp(0., 1.) * 255.) as u8)
}

fn canvas(set: &WorldSet, title: &str) -> code_texture::Canvas {
    let mut canvas = code_texture::Canvas::new(WIDTH, HEIGHT);
    let (rim, header, ink) = if set.is_library() {
        ([128, 93, 61], [55, 40, 30], [0.99, 0.89, 0.73])
    } else {
        ([25, 55, 63], [13, 31, 39], INK)
    };
    canvas.rect(0, 0, WIDTH, HEIGHT, rim);
    canvas.rect(4, 4, WIDTH - 8, HEIGHT - 8, background(set));
    canvas.rect(4, 4, WIDTH - 8, 88, header);
    canvas.text(title, 34., 64., 43., |_| ink);
    canvas
}

fn layout_chat(set: &WorldSet, shared: bool, content: &BoardContent) -> Page {
    let mut page = Page::new(if shared { "Room chat" } else { "Chat" });
    let ink = if set.is_library() {
        [0.99, 0.89, 0.73]
    } else {
        INK
    };
    for entry in &content.chat {
        page.text(&entry.login, 25., ink, false);
        page.text(&entry.text, 22., BODY, false);
        page.gap(14.);
    }
    if content.chat.is_empty() {
        let message = if shared {
            "Messages from this room appear here."
        } else if set.is_library() {
            "Open Chat on Stone Pad."
        } else {
            "Open Chat on VibePad."
        };
        page.text(message, 27., BODY, false);
    }
    page
}
