use bevy::{asset::io::embedded::EmbeddedAssetRegistry, gltf::GltfAssetLabel};
use std::path::Path;

use super::*;

pub(super) fn register(app: &App) {
    let assets = app.world().resource::<EmbeddedAssetRegistry>();
    for (name, bytes) in [
        (
            "owl.glb",
            include_bytes!("../../../../../../assets/repo-polis/owl.glb").as_slice(),
        ),
        (
            "oak.glb",
            include_bytes!("../../../../../../assets/repo-polis/oak.glb").as_slice(),
        ),
        (
            "cypress.glb",
            include_bytes!("../../../../../../assets/repo-polis/cypress.glb").as_slice(),
        ),
        (
            "column.glb",
            include_bytes!("../../../../../../assets/repo-polis/column.glb").as_slice(),
        ),
        (
            "temple.glb",
            include_bytes!("../../../../../../assets/repo-polis/temple.glb").as_slice(),
        ),
    ] {
        let path = Path::new("repo-polis").join(name);
        assets.insert_asset(path.clone(), &path, bytes);
    }
}

pub(super) fn scene(assets: &AssetServer, name: &str) -> WorldAssetRoot {
    WorldAssetRoot(
        assets.load(GltfAssetLabel::Scene(0).from_asset(format!("embedded://repo-polis/{name}"))),
    )
}
