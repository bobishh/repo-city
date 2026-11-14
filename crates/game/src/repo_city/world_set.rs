//! Presentation contract: repository identity, source and edits never depend on the set.
use bevy::prelude::*;
use serde::Deserialize;
#[derive(Resource, Clone, Deserialize)]
pub struct WorldSet {
    pub id: String,
    pub name: String,
    pub representation: String,
    pub locomotion: String,
    pub background: [f32; 3],
    pub ground: [f32; 3],
    pub shell: [f32; 3],
    pub ink: [f32; 3],
    pub accent: [f32; 3],
    pub added: [f32; 3],
    pub removed: [f32; 3],
    pub modified: [f32; 3],
    pub unchanged: [f32; 3],
}
pub fn color(v: [f32; 3]) -> Color {
    Color::srgb(v[0], v[1], v[2])
}
impl WorldSet {
    pub fn load(path: Option<&str>) -> Result<Self, String> {
        let data = match path {
            Some(p) => std::fs::read_to_string(p).map_err(|e| e.to_string())?,
            None => include_str!("../../../../world_sets/code-city.json").into(),
        };
        let set: Self = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        if set.representation != "facades" || set.locomotion != "flight-and-anchor" {
            return Err("This build supports the facades / flight-and-anchor world set. Other representations need their own presentation adapter.".into());
        }
        Ok(set)
    }
    pub fn change(&self, c: &repo_world::Change) -> Color {
        color(match c {
            repo_world::Change::Added => self.added,
            repo_world::Change::Removed => self.removed,
            repo_world::Change::Modified => self.modified,
            repo_world::Change::Unchanged => self.unchanged,
        })
    }
}
