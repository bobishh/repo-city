use std::collections::HashMap;

use rusqlite::{Connection, params};

use super::*;

fn open() -> Result<Connection, String> {
    let path = repository_picker::settings_file()
        .ok_or("Could not locate the settings folder.")?
        .with_file_name("review-notes.sqlite3");
    std::fs::create_dir_all(path.parent().expect("settings directory"))
        .map_err(|e| e.to_string())?;
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    initialize(&db)?;
    Ok(db)
}

pub(super) fn initialize(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS review (
            scope TEXT NOT NULL, id TEXT NOT NULL, payload TEXT NOT NULL, PRIMARY KEY(scope, id)
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS note_states (
            scope TEXT NOT NULL, id TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(scope, id),
            FOREIGN KEY(scope, id) REFERENCES review(scope, id) ON DELETE CASCADE
         ) WITHOUT ROWID;",
    )
    .map_err(|error| error.to_string())
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LocalState {
    Unread,
    Read,
    Addressed,
}

impl LocalState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Unread => "Unread",
            Self::Read => "Viewed",
            Self::Addressed => "Addressed",
        }
    }

    fn storage(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Read => "read",
            Self::Addressed => "addressed",
        }
    }
}

pub(super) fn states(scope: &str) -> Result<HashMap<String, LocalState>, String> {
    states_in(&open()?, scope)
}

fn states_in(db: &Connection, scope: &str) -> Result<HashMap<String, LocalState>, String> {
    let mut query = db
        .prepare("SELECT id, state FROM note_states WHERE scope = ?1")
        .map_err(|error| error.to_string())?;
    let entries = query
        .query_map([scope], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut result = HashMap::new();
    for entry in entries {
        let (id, value) = entry.map_err(|error| error.to_string())?;
        let state = match value.as_str() {
            "read" => LocalState::Read,
            "addressed" => LocalState::Addressed,
            _ => LocalState::Unread,
        };
        result.insert(id, state);
    }
    Ok(result)
}

pub(super) fn mark_local_note(scope: &str, id: &str, state: LocalState) -> Result<(), String> {
    mark_in(&open()?, scope, id, state)
}

fn mark_in(db: &Connection, scope: &str, id: &str, state: LocalState) -> Result<(), String> {
    let payload = db
        .query_row(
            "SELECT payload FROM review WHERE scope = ?1 AND id = ?2",
            params![scope, id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    let note = serde_json::from_str::<Note>(&payload).map_err(|error| error.to_string())?;
    if !is_local_note(&note) {
        return Err("The note must be a private local annotation.".into());
    }
    db.execute(
        "INSERT INTO note_states VALUES (?1, ?2, ?3)
         ON CONFLICT(scope, id) DO UPDATE SET state = excluded.state",
        params![scope, id, state.storage()],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) fn is_local_note(note: &Note) -> bool {
    !note.remote && note.published.is_none()
}

pub(super) fn local_notes(scope: &str) -> Result<Vec<(Note, LocalState)>, String> {
    notes_in(&open()?, scope)
}

fn notes_in(db: &Connection, scope: &str) -> Result<Vec<(Note, LocalState)>, String> {
    let states = states_in(db, scope)?;
    let mut query = db
        .prepare("SELECT payload FROM review WHERE scope = ?1 AND id != 'palette' ORDER BY id")
        .map_err(|error| error.to_string())?;
    let payloads = query
        .query_map([scope], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut notes = Vec::new();
    let mut seen = HashSet::new();
    for payload in payloads {
        let payload = payload.map_err(|error| error.to_string())?;
        if let Ok(note) = serde_json::from_str::<Note>(&payload)
            && is_local_note(&note)
        {
            let key =
                serde_json::to_string(&note.duplicate_key()).map_err(|error| error.to_string())?;
            if seen.insert(key) {
                let state = states.get(&note.id).copied().unwrap_or(LocalState::Unread);
                notes.push((note, state));
            }
        }
    }
    notes.reverse();
    Ok(notes)
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
    let duplicates = duplicate_drafts(&result.notes);
    if !duplicates.is_empty() {
        let transaction = db
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for id in &duplicates {
            transaction
                .execute(
                    "DELETE FROM review WHERE scope = ?1 AND id = ?2",
                    params![scope, id],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())?;
        result.notes.retain(|note| !duplicates.contains(&note.id));
    }
    Ok(result)
}

pub(super) fn save(scope: &str, id: &str, value: &impl Serialize) -> Result<(), String> {
    open()?
        .execute(
            "INSERT INTO review VALUES (?1, ?2, ?3)
             ON CONFLICT(scope, id) DO UPDATE SET payload = excluded.payload",
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

pub(super) fn save_notes(scope: &str, notes: &[Note]) -> Result<(), String> {
    let mut db = open()?;
    let transaction = db.transaction().map_err(|error| error.to_string())?;
    for note in notes {
        transaction
            .execute(
                "INSERT INTO review VALUES (?1, ?2, ?3)
             ON CONFLICT(scope, id) DO UPDATE SET payload = excluded.payload",
                params![
                    scope,
                    &note.id,
                    serde_json::to_string(note).map_err(|error| error.to_string())?
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_commit_notes_are_listed_and_addressed_without_touching_remote_notes() {
        // Given a checkout with notes on a commit, a worktree, and GitHub discussions.
        let db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        let committed = Note {
            id: "commit-note".into(),
            revision: "c9f6281fe853f3fe34546039440b38e16bc734d4".into(),
            baseline: Some("base".into()),
            anchor: Anchor {
                path: "src/file.rs".into(),
                before: false,
                start: 1,
                end: 2,
            },
            preset: defaults()[0].clone().unwrap(),
            text: "Explain the invariant.".into(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        };
        let mut live = committed.clone();
        live.id = "live-note".into();
        live.revision = "worktree:base".into();
        let mut remote = committed.clone();
        remote.id = "remote-note".into();
        remote.remote = true;
        let mut published = committed.clone();
        published.id = "published-note".into();
        published.published = Some("123".into());
        for note in [&committed, &live, &remote, &published] {
            db.execute(
                "INSERT INTO review VALUES (?1, ?2, ?3)",
                params!["checkout", note.id, serde_json::to_string(note).unwrap()],
            )
            .unwrap();
        }
        // When MCP reads local notes, both local snapshot kinds remain available.
        let notes = notes_in(&db, "checkout").unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes.iter().any(|(note, _)| note.id == committed.id));
        assert!(notes_in(&db, "another-checkout").unwrap().is_empty());
        // Then status updates apply to the commit note, never a remote discussion.
        mark_in(&db, "checkout", &committed.id, LocalState::Addressed).unwrap();
        assert!(
            notes_in(&db, "checkout")
                .unwrap()
                .iter()
                .any(|(note, state)| note.id == committed.id && *state == LocalState::Addressed)
        );
        assert!(mark_in(&db, "checkout", &remote.id, LocalState::Read).is_err());
        assert!(mark_in(&db, "checkout", &published.id, LocalState::Read).is_err());
    }

    #[test]
    fn state_changes_are_scoped_idempotent_and_keep_the_note() {
        let db = Connection::open_in_memory().unwrap();
        initialize(&db).unwrap();
        let note = Note {
            id: "note".into(),
            revision: "worktree:base".into(),
            baseline: Some("base".into()),
            anchor: Anchor {
                path: "src/file.rs".into(),
                before: false,
                start: 1,
                end: 2,
            },
            preset: defaults()[0].clone().unwrap(),
            text: "Explain the invariant.".into(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        };
        let payload = serde_json::to_string(&note).unwrap();
        db.execute("INSERT INTO review VALUES ('one', 'note', ?1)", [&payload])
            .unwrap();
        assert!(mark_in(&db, "two", "note", LocalState::Addressed).is_err());
        for _ in 0..2 {
            mark_in(&db, "one", "note", LocalState::Addressed).unwrap();
        }
        assert_eq!(
            states_in(&db, "one").unwrap()["note"],
            LocalState::Addressed
        );
        assert_eq!(
            db.query_row("SELECT payload FROM review", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            payload
        );
    }
}
