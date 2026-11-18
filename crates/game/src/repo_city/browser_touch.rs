//! Touch controls share flight and gameplay actions with physical input.
use super::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["globalThis", "repoCity"], js_name = takeTouchInput)]
    fn take_input() -> String;
}

#[derive(Default, Resource, Deserialize)]
#[serde(default)]
pub(super) struct Touch {
    pub enabled: bool,
    pub r#move: [f32; 2],
    pub look: [f32; 2],
    aim: [f32; 2],
    held: Vec<String>,
    pulses: Vec<String>,
    #[serde(skip)]
    owned: Vec<KeyCode>,
    #[serde(skip)]
    firing: bool,
}

fn key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "Tab" => KeyCode::Tab,
        "KeyM" => KeyCode::KeyM,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "KeyE" => KeyCode::KeyE,
        "KeyP" => KeyCode::KeyP,
        "KeyU" => KeyCode::KeyU,
        "KeyJ" => KeyCode::KeyJ,
        "Space" => KeyCode::Space,
        "ControlLeft" => KeyCode::ControlLeft,
        "ShiftLeft" => KeyCode::ShiftLeft,
        _ => return None,
    })
}

pub(super) fn update(
    mut touch: ResMut<Touch>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: ResMut<AccumulatedMouseScroll>,
    listing: Res<listing::Listing>,
    repo: Res<Repo>,
    explorer: Res<Explorer>,
    navigation: Res<city_navigation::Navigation>,
    camera: Query<&Transform, PlayerCamera>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let mut next = serde_json::from_str::<Touch>(&take_input()).unwrap_or_default();
    if !next
        .r#move
        .iter()
        .chain(&next.look)
        .chain(&next.aim)
        .all(|value| value.is_finite())
    {
        next = Touch::default();
    }
    if !next.enabled {
        next.r#move = [0.; 2];
        next.look = [0.; 2];
        next.aim = [0.; 2];
        next.held.clear();
        next.pulses.clear();
    }
    let reading = next.enabled
        && next.r#move.iter().all(|value| value.abs() < 0.1)
        && listing::Listing::visible(&explorer, &navigation)
        && repo.0.files.get(explorer.selected).is_some_and(|file| {
            camera
                .single()
                .ok()
                .zip(windows.single().ok())
                .is_some_and(|(camera, window)| {
                    listing
                        .close_face(
                            file,
                            camera,
                            &explorer,
                            window.width() / window.height().max(1.),
                        )
                        .is_some()
                })
        });
    if let Some(canvas) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("repo-city"))
    {
        if canvas.has_attribute("data-touch-reading") != reading {
            if reading {
                let _ = canvas.set_attribute("data-touch-reading", "");
            } else {
                let _ = canvas.remove_attribute("data-touch-reading");
            }
        }
    }
    if reading {
        next.held
            .retain(|name| name != "Space" && name != "ControlLeft");
    }
    if reading && next.look[1] != 0. {
        wheel.unit = bevy::input::mouse::MouseScrollUnit::Pixel;
        wheel.delta.y += next.look[1];
        next.look[1] = 0.;
    }
    // The look stick rotates freely; only a swipe along readable text scrolls it.
    next.look[0] += next.aim[0];
    next.look[1] += next.aim[1];
    for previous in &touch.owned {
        keys.release(*previous);
    }
    for name in next.held.iter().chain(&next.pulses).take(16) {
        if let Some(key) = key(name) {
            keys.press(key);
            next.owned.push(key);
        }
    }
    next.firing = next.enabled && next.held.iter().any(|name| name == "Fire");
    if next.firing {
        mouse.press(MouseButton::Left);
    } else if touch.firing {
        mouse.release(MouseButton::Left);
    }
    *touch = next;
}

/// Browser input uses CSS pixels. Keep Bevy's logical window in the same units,
/// leaving the window backend responsible for the physical render resolution.
pub(super) fn viewport(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    let Some(canvas) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("repo-city"))
    else {
        return;
    };
    let (width, height) = (canvas.client_width() as f32, canvas.client_height() as f32);
    if width <= 0. || height <= 0. {
        return;
    }
    if let Ok(mut window) = windows.single_mut()
        && ((window.width() - width).abs() > 0.5 || (window.height() - height).abs() > 0.5)
    {
        window.resolution.set(width, height);
    }
}
