//! A diagram staged locally or published to the current review room.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Format {
    #[default]
    Mermaid,
    Markdown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct Sketch {
    pub(super) title: String,
    pub(super) source: String,
    #[serde(default)]
    pub(super) format: Format,
}

impl Sketch {
    pub(super) fn checked(&self) -> Result<(), String> {
        if self.title.trim().is_empty()
            || self.title.len() > 240
            || self.title.contains(['\r', '\n'])
        {
            return Err("Supply a one-line title of at most 240 bytes.".into());
        }
        if self.source.trim().is_empty() || self.source.len() > 16_384 {
            return Err("Supply source of at most 16,384 bytes.".into());
        }
        Ok(())
    }

    pub(super) fn image(&self) -> Result<Image, String> {
        use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
        self.checked()?;
        let source = if self.source.trim_start().starts_with("---") {
            self.source.clone()
        } else {
            format!(
                "---\nconfig:\n  theme: dark\n  htmlLabels: false\n---\n{}",
                self.source
            )
        };
        let diagram = Renderer::new()
            .render(RenderRequest::svg(
                &source,
                OperationControl::new(),
                SvgRequest::default(),
            ))
            .map_err(|error| format!("Invalid Mermaid sketch: {error}"))?;
        let RenderOutput::Svg(Some(svg)) = diagram else {
            return Err("Supply a Mermaid flowchart or sequence diagram.".into());
        };
        image_preview::decode_named("sketch.svg", svg.svg().as_bytes())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn path(scope: &str) -> Result<PathBuf, String> {
    let directory = focus_context::directory(scope).ok_or("Could not find Repo City settings.")?;
    Ok(directory.join("sketch.json"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn stage(scope: &str, sketch: &Sketch) -> Result<(), String> {
    if !std::path::Path::new(scope).is_dir() {
        return Err("Open a local checkout to stage a sketch.".into());
    }
    sketch.checked()?;
    if sketch.format == Format::Mermaid {
        sketch.image()?;
    }
    let path = path(scope)?;
    let directory = path.parent().expect("sketch directory");
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let temporary = directory.join(format!("{}.tmp", bevy::asset::uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec(&(scope, sketch)).map_err(|error| error.to_string())?;
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    let result = std::fs::rename(&temporary, path).map_err(|error| error.to_string());
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn staged(scope: &str) -> Option<Sketch> {
    let path = path(scope).ok()?;
    if path.metadata().ok()?.len() > 110_000 {
        return None;
    }
    let (owner, sketch): (String, Sketch) =
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (owner == scope && sketch.checked().is_ok()).then_some(sketch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_mermaid_rasterizes_and_invalid_source_returns_an_error() {
        let mut sketch = Sketch {
            title: "Pipeline".into(),
            source: "flowchart LR\n A[File] --> B[City]".into(),
            format: Format::Mermaid,
        };
        let image = sketch.image().unwrap();
        assert!(image.width() > 50 && image.height() > 30);
        sketch.source = "This is not a Mermaid diagram".into();
        assert!(sketch.image().is_err());
    }
}
