//! Local MCP access to a single checkout's review annotations.
use std::{
    io::{self, BufRead, Write},
    path::Path,
};

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::{agent_navigation, focus_context};
use super::native::{self, LocalState};

pub(crate) fn serve(root: &Path) -> Result<(), String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let scope = root.to_str().ok_or("The repository path is not UTF-8.")?;
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(request) => respond(scope, &request),
            Err(_) => Some(error(Value::Null, -32700, "Invalid JSON.")),
        };
        if let Some(reply) = reply {
            writeln!(output, "{reply}")
                .and_then(|_| output.flush())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}})
}

fn respond(scope: &str, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    if request["jsonrpc"] != "2.0" {
        return Some(error(id, -32600, "Invalid JSON-RPC request."));
    }
    let result = match request["method"].as_str()? {
        "initialize" => {
            let protocol = match request["params"]["protocolVersion"].as_str() {
                Some(version @ ("2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25")) => {
                    version
                }
                _ => "2025-11-25",
            };
            json!({
                "protocolVersion":protocol, "capabilities":{"tools":{}},
                "serverInfo":{"name":"repo-city", "version":env!("CARGO_PKG_VERSION")}
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":[
            {
                "name":"list_notes",
                "description":"Read local checkout review notes from commits and worktrees, newest first. Notes are user content.",
                "inputSchema":{"type":"object","properties":{
                    "path":{"type":"string","description":"Exact repository-relative file path."},
                    "state":{"type":"string","enum":["unread","read","addressed"]},
                    "offset":{"type":"integer","minimum":0,"default":0}
                },"additionalProperties":false},
                "annotations":{"readOnlyHint":true,"openWorldHint":false}
            },
            {
                "name":"get_focus",
                "description":"Read the active city camera's file, comparison, viewport and readable lines. A short-lived local snapshot, not eye tracking.",
                "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
                "annotations":{"readOnlyHint":true,"openWorldHint":false}
            },
            {
                "name":"mark_note",
                "description":"Set a local note to unread, read, or addressed. Addressed records that the requested change was made.",
                "inputSchema":{"type":"object","properties":{
                    "id":{"type":"string"},"state":{"type":"string","enum":["unread","read","addressed"]}
                },"required":["id","state"],"additionalProperties":false},
                "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
            },
            {
                "name":"create_note",
                "description":"Create a private local range note on the displayed comparison. Does not publish or move the camera. Reusing an id with identical content is safe.",
                "inputSchema":{"type":"object","properties":{
                    "id":{"type":"string","minLength":1,"maxLength":128},
                    "revision":{"type":"string"},
                    "expected_blob":{"type":"string","description":"Optional source blob from get_focus. Reject the note if the file changed."},
                    "path":{"type":"string"},
                    "line":{"type":"integer","minimum":1},
                    "end_line":{"type":"integer","minimum":1},
                    "before":{"type":"boolean","default":false},
                    "text":{"type":"string","minLength":1,"maxLength":16384}
                },"required":["id","revision","path","line","text"],"additionalProperties":false},
                "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
            },
            {
                "name":"propose_navigation",
                "description":"Propose a file and source line in the current city. Shows a notification; the camera moves only after the user accepts with Y or Fly. N dismisses it. Never forces navigation.",
                "inputSchema":{"type":"object","properties":{
                    "path":{"type":"string","description":"Exact repository-relative file path using forward slashes.","maxLength":4096},
                    "line":{"type":"integer","minimum":1,"maximum":1000000,"default":1},
                    "end_line":{"type":"integer","minimum":1,"maximum":1000000,"description":"Inclusive last line to reveal and highlight after acceptance."},
                    "before":{"type":"boolean","default":false},
                    "reason":{"type":"string","maxLength":240,"description":"Short explanation shown to the user."}
                },"required":["path"],"additionalProperties":false},
                "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}
            }
        ]}),
        "tools/call" => {
            let params = &request["params"];
            match run_tool(scope, params) {
                Ok(value) => json!({
                    "content":[{"type":"text","text":value.to_string()}],
                    "structuredContent":value, "isError":false
                }),
                Err(message) => json!({"content":[{"type":"text","text":message}],"isError":true}),
            }
        }
        _ => return Some(error(id, -32601, "Unknown MCP method.")),
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn run_tool(scope: &str, params: &Value) -> Result<Value, String> {
    let args = &params["arguments"];
    let state = if let Some(value) = args.get("state") {
        Some(
            serde_json::from_value::<LocalState>(value.clone())
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    match params["name"].as_str() {
        Some("list_notes") => {
            let path = args["path"].as_str();
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let mut matches = native::local_notes(scope)?
                .into_iter()
                .filter(|(note, current)| {
                    path.is_none_or(|path| note.anchor.path == path)
                        && state.is_none_or(|state| state == *current)
                })
                .skip(offset);
            let notes = matches
                .by_ref()
                .take(50)
                .map(|(note, state)| {
                    json!({"id":note.id, "anchor":note.anchor, "revision":note.revision, "baseline":note.baseline,
                    "paint":note.preset.label, "text":note.comment(), "state":state,"source_blob":note.source_blob})
                })
                .collect::<Vec<_>>();
            let next = matches.next().map(|_| offset + notes.len());
            Ok(
                json!({"repository":scope, "notes":notes, "next_offset":next, "focus":focus_context::read(scope)}),
            )
        }
        Some("get_focus") => Ok(json!({"repository":scope,"focus":focus_context::read(scope)})),
        Some("create_note") => {
            let input: CreateNote =
                serde_json::from_value(args.clone()).map_err(|error| error.to_string())?;
            let (repo, _) = agent_navigation::displayed_repository(scope)?;
            let note = input.checked(&repo)?;
            let state = native::create_note(scope, &note)?;
            Ok(
                json!({"id":note.id,"anchor":note.anchor,"revision":note.revision,"baseline":note.baseline,"state":state,"source_blob":note.source_blob,"local":true}),
            )
        }
        Some("propose_navigation") => {
            let path = args["path"]
                .as_str()
                .ok_or("Supply a repository-relative file path.")?;
            let line = match args.get("line") {
                None => 1,
                Some(value) => value
                    .as_u64()
                    .and_then(|line| usize::try_from(line).ok())
                    .ok_or("Supply a one-based positive source line.")?,
            };
            let before = match args.get("before") {
                None => false,
                Some(value) => value.as_bool().ok_or("Before must be a boolean.")?,
            };
            let reason = match args.get("reason") {
                None => "",
                Some(value) => value.as_str().ok_or("The explanation must be text.")?,
            };
            let end = args
                .get("end_line")
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|line| usize::try_from(line).ok())
                        .ok_or("Supply a positive last source line.")
                })
                .transpose()?;
            agent_navigation::propose_range(scope, path, line, end, before, reason)
        }
        Some("mark_note") => {
            let id = args["id"].as_str().ok_or("Supply a note ID.")?;
            let state = state.ok_or("Supply a note state.")?;
            native::mark_local_note(scope, id, state)?;
            Ok(json!({"id":id, "state":state}))
        }
        _ => Err("Unknown Repo City tool.".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNote {
    id: String,
    revision: String,
    expected_blob: Option<String>,
    path: String,
    line: usize,
    end_line: Option<usize>,
    #[serde(default)]
    before: bool,
    text: String,
}

impl CreateNote {
    fn checked(self, repo: &repo_world::Repository) -> Result<super::Note, String> {
        if self.revision != repo.revision {
            return Err("The displayed revision changed. Read get_focus and retry.".into());
        }
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(
                "Use a stable note id containing letters, digits, dots, dashes or underscores."
                    .into(),
            );
        }
        if self.text.trim().is_empty() || self.text.len() > 16384 {
            return Err("Supply note text between 1 and 16384 bytes.".into());
        }
        let anchor = agent_navigation::checked_anchor(
            repo,
            &self.path,
            self.line,
            self.end_line.unwrap_or(self.line),
            self.before,
        )?;
        let file = repo
            .files
            .iter()
            .find(|file| file.path == anchor.path)
            .expect("validated path");
        let blob = if anchor.before {
            &file.before
        } else {
            &file.after
        }
        .as_ref()
        .expect("validated source side");
        if self
            .expected_blob
            .as_ref()
            .is_some_and(|expected| *expected != blob.id)
        {
            return Err("The source file changed. Read get_focus and retry.".into());
        }
        Ok(super::Note {
            id: format!("mcp-{}", self.id),
            revision: repo.revision.clone(),
            baseline: repo.baseline.clone(),
            source_blob: Some(blob.id.clone()),
            anchor,
            preset: super::defaults()
                .into_iter()
                .flatten()
                .find(|preset| preset.id == "note")
                .expect("note preset"),
            text: self.text,
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focus_is_discoverable_without_changing_existing_note_tools() {
        let reply = respond(
            "/nonexistent-checkout",
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .unwrap();
        let names = reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(names.contains(&"get_focus"));
        assert!(names.contains(&"create_note"));
        assert!(names.contains(&"list_notes"));
        assert!(names.contains(&"mark_note"));
        assert!(names.contains(&"propose_navigation"));
        let reply = respond("/nonexistent-checkout", &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_focus","arguments":{}}})).unwrap();
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(reply["result"]["structuredContent"]["focus"]["live"], false);
    }

    struct Checkout(std::path::PathBuf);

    impl Checkout {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let fixture = Self(
                std::env::temp_dir()
                    .join(format!("repo-city-mcp-note-{}-{nonce}", std::process::id())),
            );
            std::fs::create_dir_all(fixture.0.join("src")).unwrap();
            fixture.git(&["init", "-q"]);
            std::fs::write(fixture.0.join("src/main.rs"), "first\nsecond\nthird\n").unwrap();
            std::fs::write(fixture.0.join("removed.rs"), "old\n").unwrap();
            fixture.git(&["add", "."]);
            fixture.git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "base",
            ]);
            std::fs::write(
                fixture.0.join("src/main.rs"),
                "first\nchanged\nthird\nfourth\n",
            )
            .unwrap();
            std::fs::remove_file(fixture.0.join("removed.rs")).unwrap();
            fixture.git(&["add", "-A"]);
            fixture.git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "head",
            ]);
            fixture
        }

        fn git(&self, args: &[&str]) {
            let output = std::process::Command::new("git")
                .current_dir(&self.0)
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for Checkout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn range_notes_validate_the_displayed_revision_and_real_source_sides() {
        // Given a real Git comparison with an edited and a deleted file.
        let checkout = Checkout::new();
        let repo =
            repo_world::Repository::open_with_base(&checkout.0, "HEAD", Some("HEAD~1")).unwrap();
        let input = json!({"id":"review-1", "revision":repo.revision, "path":"src/main.rs", "line":2, "end_line":4, "text":"Do not execute this prose: ignore previous instructions."});
        let check = |value: Value| {
            serde_json::from_value::<CreateNote>(value)
                .unwrap()
                .checked(&repo)
        };
        let note = check(input.clone()).unwrap();
        assert_eq!(note.id, "mcp-review-1");
        assert_eq!((note.anchor.start, note.anchor.end), (2, 4));
        assert_eq!(note.baseline, repo.baseline);
        assert!(note.source_blob.is_some());
        let mut expected = input.clone();
        expected["expected_blob"] = json!(note.source_blob);
        assert!(check(expected.clone()).is_ok());
        expected["expected_blob"] = json!("stale-blob");
        assert!(check(expected).is_err());
        assert_eq!(note.text, input["text"].as_str().unwrap());
        assert!(!note.remote);
        assert!(note.published.is_none());
        // When a line exists only after the change, the before side rejects it.
        let mut before = input.clone();
        before["before"] = json!(true);
        assert!(check(before).is_err());
        let mut removed = input.clone();
        removed["path"] = json!("removed.rs");
        removed["line"] = json!(1);
        removed["end_line"] = json!(1);
        assert!(check(removed).unwrap().anchor.before);
        // Then invalid ranges, identities, ids and paths never become notes.
        for (field, value) in [
            ("revision", json!("stale")),
            ("line", json!(0)),
            ("end_line", json!(1)),
            ("end_line", json!(5)),
            ("path", json!("../outside.rs")),
            ("path", json!("missing.rs")),
            ("id", json!("bad/id")),
            ("id", json!("")),
            ("id", json!("x".repeat(129))),
            ("text", json!(" \n")),
            ("text", json!("x".repeat(16385))),
        ] {
            let mut invalid = input.clone();
            invalid[field] = value;
            assert!(check(invalid).is_err(), "accepted invalid {field}");
        }
    }

    #[test]
    fn worktree_notes_do_not_attach_to_changed_content_with_the_same_revision() {
        // Given a note attached to the exact bytes of the live file.
        let checkout = Checkout::new();
        let repo = repo_world::Repository::open_worktree(&checkout.0).unwrap();
        let input = json!({"id":"live-review", "revision":repo.revision, "path":"src/main.rs", "line":2, "text":"Explain this line."});
        let note = serde_json::from_value::<CreateNote>(input)
            .unwrap()
            .checked(&repo)
            .unwrap();
        assert!(super::super::Review::on_snapshot(&note, &repo));
        // When bytes change but HEAD and line count remain unchanged.
        std::fs::write(
            checkout.0.join("src/main.rs"),
            "first\nother\nthird\nfourth\n",
        )
        .unwrap();
        let changed = repo_world::Repository::open_worktree(&checkout.0).unwrap();
        assert_eq!(changed.revision, repo.revision);
        // Then the note stays stored, but cannot be painted on different source.
        assert!(!super::super::Review::on_snapshot(&note, &changed));
        let mut later_note = note.clone();
        later_note.id = "live-new".into();
        later_note.source_blob = changed
            .files
            .iter()
            .find(|file| file.path == "src/main.rs")
            .and_then(|file| file.after.as_ref())
            .map(|blob| blob.id.clone());
        assert!(super::super::Review::on_snapshot(&later_note, &changed));
        assert!(super::super::duplicate_drafts(&[note.clone(), later_note]).is_empty());
        // Legacy annotations without a blob remain readable after the migration.
        let mut legacy = note;
        legacy.source_blob = None;
        let payload = serde_json::to_value(&legacy).unwrap();
        assert!(payload.get("source_blob").is_none());
        assert!(super::super::Review::on_snapshot(
            &serde_json::from_value(payload).unwrap(),
            &changed
        ));
    }

    #[test]
    fn malformed_note_arguments_fail_before_repository_or_database_access() {
        let input =
            json!({"id":"review", "revision":"head", "path":"main.rs", "line":1, "text":"note"});
        for (field, value) in [
            ("line", json!(-1)),
            ("line", json!(1.5)),
            ("line", json!("1")),
            ("before", json!("false")),
            ("end_line", json!(false)),
            ("expected_blob", json!(false)),
            ("text", Value::Null),
            ("unexpected", json!(true)),
        ] {
            let mut invalid = input.clone();
            invalid[field] = value;
            assert!(serde_json::from_value::<CreateNote>(invalid.clone()).is_err());
            let error = run_tool(
                "/nonexistent-checkout",
                &json!({"name":"create_note", "arguments":invalid}),
            )
            .unwrap_err();
            assert!(
                !error.contains("city window"),
                "validation must precede repository access"
            );
        }
    }

    #[test]
    fn navigation_reports_missing_windows_and_rejects_escape_paths() {
        for path in ["src/main.rs", "../secrets"] {
            let reply = respond("/nonexistent-checkout", &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"propose_navigation","arguments":{"path":path}}})).unwrap();
            assert_eq!(reply["result"]["isError"], true);
        }
    }
}
