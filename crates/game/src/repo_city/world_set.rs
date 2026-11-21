//! Presentation contract: repository identity, source and edits never depend on the set.
use bevy::prelude::*;
use serde::Deserialize;

mod library;
#[cfg(not(target_arch = "wasm32"))]
pub(super) mod picker;

pub(super) type Install = fn(&mut App, &[String]);

struct Adapter {
    id: &'static str,
    descriptor: &'static str,
    representation: &'static str,
    locomotion: &'static str,
    install: Install,
}

const ADAPTERS: &[Adapter] = &[
    Adapter {
        id: "code-city",
        descriptor: include_str!("../../../../world_sets/code-city.json"),
        representation: "facades",
        locomotion: "flight-and-anchor",
        install: super::install_city,
    },
    Adapter {
        id: "greek-library",
        descriptor: include_str!("../../../../world_sets/greek-library.json"),
        representation: "shelves-and-scrolls",
        locomotion: "walking",
        install: library::install,
    },
];
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
    pub fn load(selection: Option<&str>) -> Result<Self, String> {
        let selection = selection.unwrap_or("code-city");
        if let Some(adapter) = ADAPTERS.iter().find(|adapter| adapter.id == selection) {
            return Self::parse(adapter.descriptor);
        }
        let data = std::fs::read_to_string(selection).map_err(|error| error.to_string())?;
        Self::parse(&data)
    }

    fn parse(data: &str) -> Result<Self, String> {
        let set: Self = serde_json::from_str(data).map_err(|error| error.to_string())?;
        set.adapter()?;
        Ok(set)
    }

    fn adapter(&self) -> Result<&'static Adapter, String> {
        ADAPTERS
            .iter()
            .find(|adapter| {
                adapter.representation == self.representation
                    && adapter.locomotion == self.locomotion
            })
            .ok_or_else(|| {
                format!(
                    "No world adapter for {} / {}.",
                    self.representation, self.locomotion
                )
            })
    }

    pub(super) fn installer(&self) -> Install {
        self.adapter().expect("validated world set").install
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn browser() -> Self {
        let selection = browser_world();
        let adapter = ADAPTERS
            .iter()
            .find(|adapter| adapter.id == selection)
            .unwrap_or(&ADAPTERS[0]);
        Self::parse(adapter.descriptor).expect("bundled world set")
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

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = repoCity, js_name = worldSet)]
    fn browser_world() -> String;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_explorer_can_select_the_city_or_library_by_name() {
        assert_eq!(WorldSet::load(None).unwrap().id, "code-city");
        assert_eq!(WorldSet::load(Some("code-city")).unwrap().id, "code-city");
        assert_eq!(
            WorldSet::load(Some("greek-library")).unwrap().id,
            "greek-library"
        );
    }

    #[test]
    fn a_descriptor_can_customize_a_world_but_cannot_mix_presentation_contracts() {
        let descriptor = ADAPTERS[1]
            .descriptor
            .replace("greek-library", "my-library");
        assert_eq!(
            WorldSet::parse(&descriptor).unwrap().adapter().unwrap().id,
            "greek-library"
        );
        let unsupported = descriptor.replace("walking", "flight-and-anchor");
        assert!(
            matches!(WorldSet::parse(&unsupported), Err(error) if error.contains("No world adapter"))
        );
    }
}
