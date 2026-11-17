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

pub(super) fn remove(scope: &str, id: &str) -> Result<(), String> {
    open()?
        .execute(
            "DELETE FROM review WHERE scope = ?1 AND id = ?2",
            params![scope, id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn export(scope: String, note: Note) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
    use objc2_foundation::NSString;
    let main = MainThreadMarker::new().ok_or("The file chooser needs the macOS main thread.")?;
    let panel = NSSavePanel::savePanel(main);
    panel.setNameFieldStringValue(&NSString::from_str("repo-city-review.json"));
    let opened = panel.clone();
    panel.beginWithCompletionHandler(&RcBlock::new(move |response| {
        if response != NSModalResponseOK {
            return;
        }
        let Some(path) = opened.URL().and_then(|url| url.to_file_path()) else {
            return;
        };
        let result = (|| {
            if path
                .parent()
                .and_then(|dir| dir.canonicalize().ok())
                .is_some_and(|parent| parent.starts_with(&scope))
            {
                return Err("Choose a location outside the inspected repository.".to_owned());
            }
            let bytes = serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1, "repository": scope, "revision": note.revision,
                "baseline": note.baseline, "notes": [note],
            }))
            .map_err(|e| e.to_string())?;
            std::fs::write(path, bytes).map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            warn!("Could not export review note: {error}");
        }
    }));
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn export(_: String, _: Note) -> Result<(), String> {
    Err("The review export picker is available on macOS.".into())
}
