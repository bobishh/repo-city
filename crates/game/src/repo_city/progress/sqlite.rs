use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub(super) struct Store(Connection);

impl Store {
    pub(super) fn open(path: &Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(|error| error.to_string())?;
        db.busy_timeout(std::time::Duration::from_millis(80))
            .map_err(|error| error.to_string())?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS marks (
                scope TEXT NOT NULL,
                mark TEXT NOT NULL,
                PRIMARY KEY (scope, mark)
            ) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS initialized_scopes (
                scope TEXT PRIMARY KEY
            ) WITHOUT ROWID;
            INSERT OR IGNORE INTO initialized_scopes SELECT DISTINCT scope FROM marks;
            CREATE TABLE IF NOT EXISTS last_scopes (
                checkout TEXT PRIMARY KEY,
                scope TEXT NOT NULL
            ) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS last_views (
                scope TEXT PRIMARY KEY,
                view TEXT NOT NULL
            ) WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self(db))
    }

    pub(super) fn read(&self, scope: &str) -> Result<Vec<String>, String> {
        self.read_marks(scope).map_err(|error| error.to_string())
    }

    pub(super) fn read_view(&self, scope: &str) -> Result<Option<String>, String> {
        self.0
            .query_row(
                "SELECT view FROM last_views WHERE scope = ?1",
                [scope],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    pub(super) fn write_view(&self, scope: &str, view: &str) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO last_views VALUES (?1, ?2) ON CONFLICT(scope) DO UPDATE SET view = excluded.view",
                params![scope, view],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) fn load(&mut self, scope: &str, checkout: &str) -> Result<Vec<String>, String> {
        self.carry(scope, checkout)
            .map_err(|error| error.to_string())?;
        self.read(scope)
    }

    fn carry(&mut self, scope: &str, checkout: &str) -> rusqlite::Result<()> {
        let tx = self.0.transaction()?;
        let initialized: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM initialized_scopes WHERE scope = ?1)",
            [scope],
            |row| row.get(0),
        )?;
        if !initialized {
            let previous: Option<String> = tx
                .query_row(
                    "SELECT scope FROM last_scopes WHERE checkout = ?1",
                    [checkout],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(previous) = previous {
                let mut query = tx.prepare("SELECT mark FROM marks WHERE scope = ?1")?;
                let marks = query
                    .query_map([previous], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut insert = tx.prepare("INSERT OR IGNORE INTO marks VALUES (?1, ?2)")?;
                for mark in marks {
                    if super::content::can_carry(&mark) {
                        insert.execute(params![scope, mark])?;
                    }
                }
            }
            tx.execute("INSERT INTO initialized_scopes VALUES (?1)", [scope])?;
        }
        tx.execute(
            "INSERT INTO last_scopes VALUES (?1, ?2) ON CONFLICT(checkout) DO UPDATE SET scope = excluded.scope",
            params![checkout, scope],
        )?;
        tx.commit()
    }

    fn read_marks(&self, scope: &str) -> rusqlite::Result<Vec<String>> {
        let mut query = self.0.prepare("SELECT mark FROM marks WHERE scope = ?1")?;
        query.query_map([scope], |row| row.get(0))?.collect()
    }

    pub(super) fn write(
        &mut self,
        scope: &str,
        changes: Option<&[(String, bool)]>,
    ) -> Result<(), String> {
        self.write_marks(scope, changes)
            .map_err(|error| error.to_string())
    }

    fn write_marks(
        &mut self,
        scope: &str,
        changes: Option<&[(String, bool)]>,
    ) -> rusqlite::Result<()> {
        let tx = self.0.transaction()?;
        if let Some(changes) = changes {
            let mut insert = tx.prepare("INSERT OR IGNORE INTO marks VALUES (?1, ?2)")?;
            let mut delete = tx.prepare("DELETE FROM marks WHERE scope = ?1 AND mark = ?2")?;
            for (mark, checked) in changes {
                if *checked {
                    insert.execute(params![scope, mark])?;
                } else {
                    delete.execute(params![scope, mark])?;
                }
            }
        } else {
            tx.execute("DELETE FROM marks WHERE scope = ?1", [scope])?;
        }
        // An explicitly empty scope remains empty when reopened after another comparison.
        tx.execute(
            "INSERT OR IGNORE INTO initialized_scopes VALUES (?1)",
            [scope],
        )?;
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_only_bound_read_credit_within_a_checkout_and_keeps_reset_scopes_empty() {
        let mut store = Store::open(Path::new(":memory:")).unwrap();
        let reveal = r#"{"reveal":{"path":"code.rs","before":false,"start":0,"end":1,"rows":1},"source":{"blob":"old","lines":["read"]}}"#;
        let document = r#"{"read_document":{"path":"guide.md","before":false,"rows":1,"blob":"old"},"source":{"blob":"old","lines":["paragraph"]}}"#;
        let legacy = r#"{"reveal":{"path":"legacy.rs","before":false,"start":0,"end":1,"rows":1}}"#;
        assert!(store.load("first", "checkout").unwrap().is_empty());
        store
            .write(
                "first",
                Some(&[
                    (reveal.into(), true),
                    (document.into(), true),
                    (legacy.into(), true),
                    (r#"{"reviewed":"code.rs"}"#.into(), true),
                    (r#"{"visited":{"path":"code.rs","diff":true}}"#.into(), true),
                ]),
            )
            .unwrap();
        let mut carried = store.load("second", "checkout").unwrap();
        carried.sort();
        let mut expected = vec![reveal, document];
        expected.sort();
        assert_eq!(carried, expected);
        assert!(
            store
                .load("other", "different-checkout")
                .unwrap()
                .is_empty()
        );
        store.write("second", None).unwrap();
        assert!(!store.load("first", "checkout").unwrap().is_empty());
        assert!(store.load("second", "checkout").unwrap().is_empty());
        assert!(store.load("third", "checkout").unwrap().is_empty());
    }

    #[test]
    fn restores_marks_after_reopen_and_resets_only_the_active_comparison() {
        let path =
            std::env::temp_dir().join(format!("repo-city-progress-{}.sqlite3", std::process::id()));
        let mut first = Store::open(&path).unwrap();
        first
            .write(
                "first",
                Some(&[("reveal".into(), true), ("reviewed".into(), true)]),
            )
            .unwrap();
        first
            .write("second", Some(&[("other".into(), true)]))
            .unwrap();
        drop(first);

        let mut reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.read("first").unwrap(), ["reveal", "reviewed"]);
        reopened
            .write("first", Some(&[("reviewed".into(), false)]))
            .unwrap();
        assert_eq!(reopened.read("first").unwrap(), ["reveal"]);
        reopened.write("first", None).unwrap();
        assert!(reopened.read("first").unwrap().is_empty());
        assert_eq!(reopened.read("second").unwrap(), ["other"]);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
}
