//! Shared semantic navigation and camera projection for an authorized review room.
//! The service owns membership; each client owns its source, progress and drafts.
use super::*;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
pub(super) mod chat;
#[cfg(not(target_arch = "wasm32"))]
mod media;
#[cfg(not(target_arch = "wasm32"))]
mod native;

pub(super) struct Plugin;
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct ProjectRoom;
impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Party>()
            .init_resource::<Markers>()
            .add_systems(Update, project.in_set(ProjectRoom).after(listing::draw))
            .add_systems(Update, draw_peers.after(project));
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::register(app);
            chat::register(app);
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Focus {
    pub path: String,
    pub face: String,
    #[serde(default)]
    pub before: bool,
    #[serde(default = "first_line")]
    pub line: usize,
    #[serde(default = "first_line")]
    pub end_line: usize,
}
fn first_line() -> usize {
    1
}

impl Focus {
    fn checked(&self, repo: &Repository) -> Option<listing::Face> {
        if self.path.is_empty()
            || self.path.starts_with('/')
            || self.path.contains(['\\', '\0', '\n', '\r'])
            || self.path.split('/').any(|part| part == "." || part == "..")
            || self.line == 0
            || self.end_line < self.line
            || self.end_line > 1_000_000
        {
            return None;
        }
        let file = repo.files.iter().find(|file| file.path == self.path)?;
        (if self.before {
            file.before.as_ref()
        } else {
            file.after.as_ref()
        })?;
        match self.face.as_str() {
            "source" => Some(listing::Face::Source),
            "diff" => Some(listing::Face::Diff),
            "blame" => Some(listing::Face::Blame),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize)]
struct Pose {
    position: [f32; 3],
    forward: [f32; 3],
}

#[derive(Clone, Serialize)]
struct Projection {
    pose: Option<Pose>,
    focus: Option<Focus>,
}

#[derive(Resource, Default)]
pub(super) struct Scene {
    pub focus: Option<Focus>,
    pub positions: std::collections::HashMap<String, Vec3>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub(super) struct BoardContent {
    pub(super) chat: Vec<BoardMessage>,
    pub(super) sketch: Option<sketch::Sketch>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct BoardMessage {
    pub(super) login: String,
    pub(super) text: String,
}

#[derive(Resource, Default)]
pub(super) struct Party {
    destination: Option<Focus>,
    projection: Option<Projection>,
    next: f64,
    peers: Vec<serde_json::Value>,
    #[cfg(target_arch = "wasm32")]
    board: Option<BoardContent>,
    #[cfg(not(target_arch = "wasm32"))]
    native: native::State,
}
impl Party {
    pub(super) fn board_content(&self) -> Option<BoardContent> {
        #[cfg(target_arch = "wasm32")]
        return self.board.clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.native
            .connected()
            .then(|| self.native.chat.board_content())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn chat_room(&self) -> Option<&str> {
        self.native.chat_room()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn hosts_room(&self) -> bool {
        self.native.hosts_room()
    }

    pub(super) fn take_destination(
        &mut self,
        repo: &Repository,
    ) -> Option<(review_notes::Destination, listing::Face)> {
        let focus = self.destination.take()?;
        let face = focus.checked(repo)?;
        Some((
            review_notes::Destination {
                revision: repo.revision.clone(),
                anchor: review_notes::Anchor {
                    path: focus.path,
                    before: focus.before,
                    start: focus.line,
                    end: focus.end_line,
                },
            },
            face,
        ))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = partyUpdate)]
    fn party_update(value: &str);
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takePartyDestination)]
    fn take_party_destination() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = partyActive)]
    fn party_active() -> bool;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = partyPeers)]
    fn party_peers() -> String;
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takePartyBoard)]
    fn take_party_board() -> String;
}

fn project(
    time: Res<Time>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    listing: Res<listing::Listing>,
    mut party: ResMut<Party>,
    mut review: ResMut<review_notes::Review>,
    camera: Query<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
    scene: Option<Res<Scene>>,
) {
    #[cfg(target_arch = "wasm32")]
    let active = party_active();
    #[cfg(not(target_arch = "wasm32"))]
    let active = party.native.connected();
    if !active {
        party.peers.clear();
        #[cfg(target_arch = "wasm32")]
        {
            party.board = None;
        }
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        party.peers = serde_json::from_str(&party_peers()).unwrap_or_default();
        let board = take_party_board();
        if !board.is_empty() {
            party.board = serde_json::from_str(&board).ok();
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        party.peers = party.native.peers();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        review.auto_publish = false;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let target = take_party_destination();
        if !target.is_empty() {
            party.destination = serde_json::from_str(&target).ok();
        }
    }
    if time.elapsed_secs_f64() < party.next {
        return;
    }
    party.next = time.elapsed_secs_f64() + 0.25;
    let (Ok(camera), Ok(window)) = (camera.single(), windows.single()) else {
        return;
    };
    let aspect = window.width() / window.height().max(1.);
    let focus = if let Some(scene) = &scene {
        scene.focus.clone()
    } else {
        repo.0
            .files
            .get(explorer.selected)
            .filter(|_| window.focused && !review.blocked && !review.tablet_open)
            .and_then(|file| {
                let face = listing.close_face(file, camera, &explorer, aspect)?;
                let rows = listing.visible_source_rows(file, camera, &explorer, aspect);
                Some(Focus {
                    path: file.path.clone(),
                    before: explorer.before,
                    face: match face {
                        listing::Face::Source => "source",
                        listing::Face::Diff => "diff",
                        listing::Face::Blame => "blame",
                    }
                    .into(),
                    line: rows.as_ref().map_or(1, |range| range.start + 1),
                    end_line: rows.map_or(1, |range| range.end.max(1)),
                })
            })
    };
    let projection = Projection {
        pose: scene.is_none().then(|| Pose {
            position: camera.translation.to_array(),
            forward: camera.forward().to_array(),
        }),
        focus,
    };
    #[cfg(target_arch = "wasm32")]
    if let Ok(value) = serde_json::to_string(&projection) {
        party_update(&value);
    }
    party.projection = Some(projection);
}

#[derive(Component)]
struct Marker;
#[derive(Resource, Default)]
struct Markers {
    entities: std::collections::HashMap<(String, bool), Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
}
fn draw_peers(
    mut commands: Commands,
    party: Res<Party>,
    repo: Res<Repo>,
    mut markers: ResMut<Markers>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform, With<Marker>>,
    scene: Option<Res<Scene>>,
    set: Res<WorldSet>,
) {
    let mut targets = std::collections::HashMap::new();
    for peer in &party.peers {
        let Some(id) = peer["id"].as_str() else {
            continue;
        };
        let name = peer["login"].as_str().unwrap_or("Reviewer");
        if scene.is_none()
            && let Some(position) = peer["pose"]["position"]
                .as_array()
                .filter(|value| value.len() == 3)
        {
            let coordinates = position
                .iter()
                .filter_map(serde_json::Value::as_f64)
                .map(|value| value as f32)
                .collect::<Vec<_>>();
            if coordinates.len() == 3
                && coordinates
                    .iter()
                    .all(|value| value.is_finite() && value.abs() <= 1e8)
            {
                targets.insert(
                    (id.to_owned(), false),
                    (
                        name.to_owned(),
                        Vec3::new(coordinates[0], coordinates[1], coordinates[2]),
                    ),
                );
            }
        }
        if let Some(path) = peer["focus"]["path"].as_str()
            && let Some(file) = repo.0.files.iter().find(|file| file.path == path)
        {
            targets.insert(
                (id.to_owned(), true),
                (
                    format!("{name} reading {path}"),
                    if let Some(scene) = &scene {
                        let Some(position) = scene.positions.get(path) else {
                            continue;
                        };
                        *position
                    } else {
                        Vec3::new(
                            file.position[0],
                            listing::block_size(file).y + 3.,
                            file.position[1],
                        )
                    },
                ),
            );
        }
    }
    let removed = markers
        .entities
        .keys()
        .filter(|key| !targets.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    for key in removed {
        if let Some(entity) = markers.entities.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    if targets.is_empty() {
        return;
    }
    let mesh = markers
        .mesh
        .get_or_insert_with(|| meshes.add(Cuboid::new(1.2, 2., 1.2)))
        .clone();
    let material = markers
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: if scene.is_some() {
                    color(set.accent)
                } else {
                    Color::srgb_u8(90, 232, 225)
                },
                emissive: if scene.is_some() {
                    Color::srgb(0.42, 0.22, 0.07)
                } else {
                    Color::srgb(0.15, 0.5, 0.48)
                }
                .into(),
                ..default()
            })
        })
        .clone();
    for (key, (name, position)) in targets {
        if let Some(entity) = markers.entities.get(&key) {
            if let Ok(mut transform) = transforms.get_mut(*entity) {
                transform.translation = transform.translation.lerp(position, 0.25);
            }
        } else {
            let entity = commands
                .spawn((
                    Marker,
                    Name::new(name),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(position),
                ))
                .id();
            markers.entities.insert(key, entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_cues_resolve_only_available_snapshot_paths_and_faces() {
        let repo = Repository {
            root: PathBuf::from("/fixture"),
            revision: "head".into(),
            baseline: None,
            files: vec![repo_world::File {
                path: "test.rs".into(),
                before: None,
                after: Some(repo_world::Blob {
                    id: "blob".into(),
                    size: 20,
                    mode: "100644".into(),
                }),
                change: Change::Added,
                position: [0., 0.],
            }],
            districts: vec![],
        };
        let mut party = Party::default();
        let path = repo.files[0].path.clone();
        party.destination = Some(Focus {
            path: path.clone(),
            face: "blame".into(),
            before: false,
            line: 3,
            end_line: 5,
        });
        let (destination, face) = party.take_destination(&repo).unwrap();
        assert_eq!(destination.anchor.start, 3);
        assert_eq!(destination.anchor.end, 5);
        assert_eq!(face, listing::Face::Blame);
        assert!(party.take_destination(&repo).is_none());
        for (path, face, line, end_line) in [
            ("../secret", "source", 1, 1),
            (path.as_str(), "roof", 1, 1),
            (path.as_str(), "source", 0, 1),
            (path.as_str(), "source", 3, 2),
        ] {
            party.destination = Some(Focus {
                path: path.into(),
                face: face.into(),
                before: false,
                line,
                end_line,
            });
            assert!(party.take_destination(&repo).is_none());
        }
    }
}
