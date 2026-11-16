use std::path::Path;

use rusqlite::{Connection, params};

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
            ) WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
        Ok(Self(db))
    }

    pub(super) fn read(&self, scope: &str) -> Result<Vec<String>, String> {
        self.read_marks(scope).map_err(|error| error.to_string())
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
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
