use rusqlite::{Connection, params};

use super::*;

fn open() -> Result<Connection, String> {
    let path = repository_picker::settings_file()
        .ok_or("Could not locate the settings folder.")?
        .with_file_name("review-notes.sqlite3");
    std::fs::create_dir_all(path.parent().expect("settings directory"))
        .map_err(|e| e.to_string())?;
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS review (
        scope TEXT NOT NULL, id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(scope, id)
    ) WITHOUT ROWID;",
    )
    .map_err(|e| e.to_string())?;
    Ok(db)
}

pub(super) fn load(scope: &str) -> Result<Payload, String> {
    let db = open()?;
    let mut statement = db
        .prepare("SELECT id, payload FROM review WHERE scope = ?1 ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Payload {
        palette: defaults(),
        notes: vec![],
    };
    for row in rows {
        let (id, value) = row.map_err(|e| e.to_string())?;
        if id == "palette" {
            if let Ok(palette) = serde_json::from_str::<Vec<Option<Preset>>>(&value)
                && palette.len() == 9
            {
                result.palette = palette;
            }
        } else if let Ok(note) = serde_json::from_str(&value) {
            result.notes.push(note);
        }
    }
    Ok(result)
}

pub(super) fn save(scope: &str, id: &str, value: &impl Serialize) -> Result<(), String> {
    open()?
        .execute(
            "INSERT OR REPLACE INTO review VALUES (?1, ?2, ?3)",
            params![
                scope,
                id,
                serde_json::to_string(value).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn panel(review: &mut Review, panel: Panel) -> Result<(), String> {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertThirdButtonReturn, NSPopUpButton, NSTextField,
        NSView,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    let main = MainThreadMarker::new().ok_or("Review editor needs the macOS main thread.")?;
    let rect = |x, y, w, h| NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
    let string = |value: &str| NSString::from_str(value);
    let alert = NSAlert::new(main);
    match panel {
        Panel::Paints => {
            let choices = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(main),
                rect(0., 0., 440., 30.),
                false,
            );
            choices.addItemWithTitle(&string("0 · Water"));
            for (index, preset) in review.palette.iter().enumerate() {
                choices.addItemWithTitle(&string(&format!(
                    "{} · {}",
                    index + 1,
                    preset.as_ref().map_or("Empty", |p| p.label.as_str())
                )));
            }
            choices.selectItemAtIndex(review.slot as isize);
            alert.setMessageText(&string("Paints"));
            alert.setAccessoryView(Some(&choices));
            alert.addButtonWithTitle(&string("Equip"));
            alert.addButtonWithTitle(&string("Cancel"));
            alert.addButtonWithTitle(&string("Edit paint…"));
            let result = alert.runModal();
            let selected = choices.indexOfSelectedItem() as usize;
            if result == NSAlertFirstButtonReturn {
                review.select(selected);
            } else if result == NSAlertThirdButtonReturn && (1..=9).contains(&selected) {
                let current = review.palette[selected - 1]
                    .clone()
                    .unwrap_or_else(|| Preset {
                        id: new_id(),
                        label: String::new(),
                        spray: String::new(),
                        prompt: String::new(),
                        color: [255, 126, 141],
                    });
                let editor = NSAlert::new(main);
                editor.setMessageText(&string("Edit paint"));
                editor.addButtonWithTitle(&string("Save"));
                editor.addButtonWithTitle(&string("Cancel"));
                let view = NSView::initWithFrame(NSView::alloc(main), rect(0., 0., 440., 186.));
                let [r, g, b] = current.color;
                let values = [
                    current.label.clone(),
                    current.spray.clone(),
                    current.prompt.clone(),
                    format!("#{r:02x}{g:02x}{b:02x}"),
                    selected.to_string(),
                ];
                let labels = [
                    "Name",
                    "Spray text",
                    "Reusable prompt",
                    "Color (#RRGGBB)",
                    "Slot (1–9)",
                ];
                let fields: Vec<_> = labels
                    .into_iter()
                    .zip(&values)
                    .enumerate()
                    .map(|(i, (name, value))| {
                        let y = 149. - i as f64 * 35.;
                        let label = NSTextField::labelWithString(&string(name), main);
                        label.setFrame(rect(0., y, 135., 26.));
                        view.addSubview(&label);
                        let field = NSTextField::textFieldWithString(&string(value), main);
                        field.setFrame(rect(140., y, 300., 26.));
                        view.addSubview(&field);
                        field
                    })
                    .collect();
                editor.setAccessoryView(Some(&view));
                if editor.runModal() != NSAlertFirstButtonReturn {
                    return Ok(());
                }
                let value = |index: usize| fields[index].stringValue().to_string();
                let hex = value(3);
                let hex = hex
                    .strip_prefix('#')
                    .filter(|s| s.len() == 6 && s.is_ascii())
                    .ok_or("Enter a color as #RRGGBB.")?;
                let byte = |start| {
                    u8::from_str_radix(&hex[start..start + 2], 16)
                        .map_err(|_| "Enter a color as #RRGGBB.")
                };
                let slot = value(4)
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=9).contains(n))
                    .ok_or("Choose slot 1 through 9.")?;
                let label = value(0).trim().to_owned();
                if label.is_empty() {
                    return Err("Give the paint a name.".into());
                }
                let mut palette = review.palette.clone();
                if slot != selected {
                    palette.swap(slot - 1, selected - 1);
                }
                palette[slot - 1] = Some(Preset {
                    label,
                    spray: value(1),
                    prompt: value(2),
                    color: [byte(0)?, byte(2)?, byte(4)?],
                    ..current
                });
                save(&review.scope, "palette", &palette)?;
                review.palette = palette;
                review.select(slot);
            }
        }
        Panel::Notes | Panel::Aimed => {
            let id = if matches!(panel, Panel::Aimed) {
                review.aim_note.as_deref()
            } else {
                None
            };
            let index = if let Some(id) = id {
                review.notes.iter().position(|note| note.id == id)
            } else {
                let choices = NSPopUpButton::initWithFrame_pullsDown(
                    NSPopUpButton::alloc(main),
                    rect(0., 0., 500., 30.),
                    false,
                );
                for note in &review.notes {
                    choices.addItemWithTitle(&string(&format!(
                        "{} · {}:{}–{} · {}",
                        note.preset.label,
                        note.anchor.path,
                        note.anchor.start,
                        note.anchor.end,
                        &note.revision[..note.revision.len().min(7)]
                    )));
                }
                alert.setMessageText(&string("Review notes"));
                alert.setAccessoryView(Some(&choices));
                alert.addButtonWithTitle(&string("Open note"));
                alert.addButtonWithTitle(&string("Close"));
                alert.addButtonWithTitle(&string("Export selected note…"));
                match alert.runModal() {
                    value if value == NSAlertFirstButtonReturn => {
                        Some(choices.indexOfSelectedItem() as usize)
                    }
                    value if value == NSAlertThirdButtonReturn => {
                        use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
                        let save_panel = NSSavePanel::savePanel(main);
                        save_panel.setNameFieldStringValue(&string("repo-city-review.json"));
                        if save_panel.runModal() == NSModalResponseOK
                            && let Some(path) = save_panel.URL().and_then(|url| url.to_file_path())
                        {
                            let parent = path.parent().and_then(|dir| dir.canonicalize().ok());
                            if parent.is_some_and(|dir| dir.starts_with(&review.scope)) {
                                return Err(
                                    "Choose a location outside the inspected repository.".into()
                                );
                            }
                            let selected = review
                                .notes
                                .get(choices.indexOfSelectedItem() as usize)
                                .ok_or("Select a note to export.")?;
                            let bytes = serde_json::to_vec_pretty(&serde_json::json!({
                                "version": 1, "repository": review.scope,
                                "revision": selected.revision, "baseline": selected.baseline, "notes": [selected],
                            }))
                            .map_err(|e| e.to_string())?;
                            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
                        }
                        None
                    }
                    _ => None,
                }
            };
            if let Some(index) = index
                && let Some(note) = review.notes.get(index)
            {
                let editor = NSAlert::new(main);
                editor.setMessageText(&string(&format!(
                    "{}:{}–{}",
                    note.anchor.path, note.anchor.start, note.anchor.end
                )));
                editor.setInformativeText(&string(&format!(
                    "{} · {}\n{}",
                    note.preset.label,
                    &note.revision[..note.revision.len().min(7)],
                    note.preset.prompt
                )));
                editor.addButtonWithTitle(&string("Save note"));
                editor.addButtonWithTitle(&string("Cancel"));
                editor.addButtonWithTitle(&string("Delete draft"));
                let field = NSTextField::textFieldWithString(&string(&note.text), main);
                field.setFrame(rect(0., 0., 490., 30.));
                let view = NSView::initWithFrame(NSView::alloc(main), rect(0., 0., 490., 65.));
                view.addSubview(&field);
                let choices = NSPopUpButton::initWithFrame_pullsDown(
                    NSPopUpButton::alloc(main),
                    rect(0., 34., 490., 30.),
                    false,
                );
                let paints: Vec<_> = std::iter::once(&note.preset)
                    .chain(
                        review
                            .palette
                            .iter()
                            .flatten()
                            .filter(|paint| paint.id != note.preset.id),
                    )
                    .collect();
                for paint in &paints {
                    choices.addItemWithTitle(&string(&paint.label));
                }
                view.addSubview(&choices);
                editor.setAccessoryView(Some(&view));
                match editor.runModal() {
                    value if value == NSAlertFirstButtonReturn => {
                        let mut edited = note.clone();
                        edited.text = field.stringValue().to_string();
                        if let Some(paint) = paints.get(choices.indexOfSelectedItem() as usize) {
                            edited.preset = (*paint).clone();
                        }
                        save(&review.scope, &edited.id, &edited)?;
                        review.notes[index] = edited;
                    }
                    value if value == NSAlertThirdButtonReturn => {
                        open()?
                            .execute(
                                "DELETE FROM review WHERE scope = ?1 AND id = ?2",
                                params![review.scope, note.id],
                            )
                            .map_err(|e| e.to_string())?;
                        review.notes.remove(index);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn panel(_: &mut Review, _: Panel) -> Result<(), String> {
    Err("The native review editor is available on macOS.".into())
}
