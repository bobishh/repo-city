//! A GitHub snapshot uses an app-owned bare repository and the user’s GitHub session.
use std::{
    path::Path,
    process::{Command, Output},
    sync::{Mutex, mpsc},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{PathBuf, Repository, github_choices::Choice, repository_picker, review_notes::Note};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    owner: String,
    name: String,
    pull: Option<u64>,
}

impl Target {
    pub fn parse(input: &str) -> Result<Self, String> {
        let input = input.trim();
        let input = input.split(['?', '#']).next().unwrap_or(input);
        let path = input.strip_prefix("https://github.com/").unwrap_or(input);
        let parts = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
        let component = |value: &str| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        };
        let invalid = || "Enter owner/repository or a GitHub pull request URL.".to_owned();
        let (owner, name, pull) = match parts.as_slice() {
            [owner, name] => (*owner, *name, None),
            [owner, name, "pull", number]
            | [owner, name, "pull", number, "files" | "commits" | "checks"] => (
                *owner,
                *name,
                Some(
                    number
                        .parse::<u64>()
                        .ok()
                        .filter(|id| *id > 0)
                        .ok_or_else(invalid)?,
                ),
            ),
            _ => return Err(invalid()),
        };
        let name = name.strip_suffix(".git").unwrap_or(name);
        if !component(owner) || !component(name) {
            return Err(invalid());
        }
        Ok(Self {
            owner: owner.to_ascii_lowercase(),
            name: name.to_ascii_lowercase(),
            pull,
        })
    }

    fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn label(&self) -> String {
        self.pull
            .map_or_else(|| self.slug(), |number| format!("{}#{number}", self.slug()))
    }

    pub fn url(&self) -> String {
        let repo = format!("https://github.com/{}", self.slug());
        self.pull
            .map_or_else(|| repo.clone(), |number| format!("{repo}/pull/{number}"))
    }

    pub fn is_pull(&self) -> bool {
        self.pull.is_some()
    }

    fn pull_endpoint(&self) -> Option<String> {
        self.pull
            .map(|number| format!("repos/{}/pulls/{number}", self.slug()))
    }

    fn cache(&self) -> Result<PathBuf, String> {
        let settings = repository_picker::settings_file().ok_or("Could not find app storage.")?;
        let snapshot = self
            .pull
            .map_or_else(|| "repository.git".into(), |id| format!("pr-{id}.git"));
        Ok(settings
            .with_file_name("github")
            .join(&self.owner)
            .join(&self.name)
            .join(snapshot))
    }
}

#[derive(Deserialize)]
struct Pull {
    head: Commit,
    base: Commit,
    state: String,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

#[derive(Deserialize)]
struct Comment {
    html_url: String,
    #[serde(default)]
    body: String,
}

pub(super) struct Session {
    token: String,
    pub login: String,
}

const CLIENT_ID: &str = "Iv23liflIsPOavfEGn6b";
const DEVICE_URL: &str = "https://github.com/login/device";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(25))
        .user_agent("Repo City")
        .build()
}

fn json_response<T: serde::de::DeserializeOwned>(
    response: Result<ureq::Response, ureq::Error>,
) -> Result<T, String> {
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let message = response
                .into_json::<Value>()
                .ok()
                .and_then(|value| value["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "The request failed.".into());
            return Err(format!("GitHub {status}: {message}"));
        }
        Err(error) => return Err(error.to_string()),
    };
    response.into_json().map_err(|error| error.to_string())
}

fn oauth_response(response: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    match response {
        Err(ureq::Error::Status(400, response)) => json_response(Ok(response)),
        response => json_response(response),
    }
}

fn api<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    token: Option<&str>,
    body: Option<&Value>,
) -> Result<T, String> {
    api_query(endpoint, token, body, &[])
}

fn api_query<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    token: Option<&str>,
    body: Option<&Value>,
    query: &[(&str, &str)],
) -> Result<T, String> {
    let client = agent();
    let url = format!("https://api.github.com/{endpoint}");
    let request = client
        .request(if body.is_some() { "POST" } else { "GET" }, &url)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28");
    let request = match token {
        Some(token) => request.set("Authorization", &format!("Bearer {token}")),
        None => request,
    };
    let request = query
        .iter()
        .fold(request, |request, (key, value)| request.query(key, value));
    json_response(match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    })
}

fn commit_choice(commit: &Value) -> Option<Choice> {
    let value = sha(commit["sha"].as_str()?).ok()?;
    let title = commit["commit"]["message"].as_str()?.lines().next()?;
    Some(Choice {
        value: value.into(),
        label: format!("{} | {title}", &value[..7]),
    })
}

pub(super) fn revisions(target: Target, token: Option<String>) -> Job<Vec<Choice>> {
    Job::spawn(move || {
        let path = format!("repos/{}", target.slug());
        let endpoints = [
            "branches?per_page=100",
            "tags?per_page=50",
            "commits?per_page=30",
        ];
        let [branches, tags, commits] = std::thread::scope(|scope| {
            endpoints
                .map(|endpoint| {
                    let endpoint = format!("{path}/{endpoint}");
                    let token = token.as_deref();
                    scope.spawn(move || api::<Vec<Value>>(&endpoint, token, None))
                })
                .map(|job| job.join().unwrap())
        });
        if branches.is_err() && tags.is_err() && commits.is_err() {
            return Err("GitHub revisions unavailable.".into());
        }
        let mut choices = Vec::new();
        for (items, kind, prefix) in [(branches, "Branch", "heads"), (tags, "Tag", "tags")] {
            choices.extend(items.unwrap_or_default().iter().filter_map(|item| {
                let name = item["name"].as_str()?;
                Some(Choice {
                    value: format!("refs/{prefix}/{name}"),
                    label: format!("{kind} | {name}"),
                })
            }));
        }
        choices.extend(commits.unwrap_or_default().iter().filter_map(commit_choice));
        Ok(choices)
    })
}

pub(super) fn search_commits(
    target: Target,
    query: String,
    token: Option<String>,
) -> Job<Vec<Choice>> {
    Job::spawn(move || {
        let slug = target.slug();
        let words = query.chars().take(80).collect::<String>();
        let words = words
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|word| !word.is_empty())
            .take(8)
            .map(|word| format!("\"{word}\""))
            .collect::<Vec<_>>()
            .join(" ");
        if words.len() < 5 {
            return Ok(vec![]);
        }
        let value: Value = api_query(
            "search/commits",
            token.as_deref(),
            None,
            &[("q", &format!("repo:{slug} {words}")), ("per_page", "8")],
        )?;
        let items = value["items"]
            .as_array()
            .ok_or("GitHub commit search unavailable.")?;
        Ok(items
            .iter()
            .filter(|item| {
                item["repository"]["full_name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&slug))
            })
            .filter_map(commit_choice)
            .collect())
    })
}

fn pages(
    path: &str,
    token: Option<&str>,
    field: Option<&str>,
    max_pages: usize,
) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let separator = if path.contains('?') { '&' } else { '?' };
    for page in 1..=max_pages {
        let result: Value = api(
            &format!("{path}{separator}per_page=100&page={page}"),
            token,
            None,
        )?;
        let batch = field
            .map_or(&result, |field| &result[field])
            .as_array()
            .ok_or("GitHub returned an invalid list.")?;
        items.extend(batch.iter().cloned());
        if batch.len() < 100 {
            break;
        }
    }
    Ok(items)
}

pub(super) fn library(login: String, token: String) -> Job<Vec<Choice>> {
    Job::spawn(move || {
        let (own, stars, installations) = std::thread::scope(|scope| {
            let own = scope.spawn(|| {
                pages(
                    &format!("users/{login}/repos?sort=pushed"),
                    Some(&token),
                    None,
                    10,
                )
            });
            let stars = scope.spawn(|| pages("user/starred", Some(&token), None, 10));
            let installations = pages(
                "user/installations",
                Some(&token),
                Some("installations"),
                10,
            );
            (own.join().unwrap(), stars.join().unwrap(), installations)
        });
        let mut repos = own.as_ref().cloned().unwrap_or_default();
        for installation in installations.as_deref().unwrap_or_default() {
            if let Some(id) = installation["id"].as_u64()
                && let Ok(granted) = pages(
                    &format!("user/installations/{id}/repositories"),
                    Some(&token),
                    Some("repositories"),
                    10,
                )
            {
                repos.extend(granted);
            }
        }
        if repos.is_empty() && stars.is_err() && (own.is_err() || installations.is_err()) {
            return Err("GitHub repositories unavailable.".into());
        }
        let mut seen = std::collections::HashSet::new();
        let mut entries = Vec::new();
        repos.sort_by_key(|repo| {
            (
                repo["fork"].as_bool().unwrap_or(false),
                repo["full_name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
            )
        });
        let mut stars = stars.unwrap_or_default();
        stars.sort_by_key(|repo| {
            repo["full_name"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
        });
        for (repo, starred) in repos
            .into_iter()
            .map(|repo| (repo, false))
            .chain(stars.into_iter().map(|repo| (repo, true)))
        {
            let Some(value) = repo["full_name"].as_str() else {
                continue;
            };
            if !seen.insert(value.to_ascii_lowercase()) {
                continue;
            }
            let private = if repo["private"].as_bool() == Some(true) {
                " | Private"
            } else {
                ""
            };
            let category = if starred {
                " | Star"
            } else if repo["fork"].as_bool() == Some(true) {
                " | Fork"
            } else {
                ""
            };
            entries.push(Choice {
                value: value.into(),
                label: format!("{value}{private}{category}"),
            });
        }
        Ok(entries)
    })
}

pub(super) fn pull_requests(target: Target, token: Option<String>) -> Job<Vec<Choice>> {
    Job::spawn(move || {
        let items = pages(
            &format!("repos/{}/pulls?state=all", target.slug()),
            token.as_deref(),
            None,
            2,
        )?;
        Ok(items
            .iter()
            .filter_map(|pull| {
                let number = pull["number"].as_u64()?;
                let title = pull["title"].as_str()?;
                let state = match pull["state"].as_str()? {
                    "open" => "Open",
                    "closed" if pull["merged_at"].is_null() => "Closed",
                    "closed" => "Merged",
                    _ => return None,
                };
                let author = pull["user"]["login"].as_str().unwrap_or("unknown");
                Some(Choice {
                    value: format!("https://github.com/{}/pull/{number}", target.slug()),
                    label: format!("#{number} | {state} | {title} | @{author}"),
                })
            })
            .collect())
    })
}

#[derive(Deserialize)]
pub(super) struct DeviceCode {
    device_code: String,
    pub user_code: String,
    expires_in: u64,
    interval: u64,
}

pub(super) fn request_sign_in() -> Job<DeviceCode> {
    Job::spawn(|| {
        let result = oauth_response(
            agent()
                .post("https://github.com/login/device/code")
                .set("Accept", "application/json")
                .send_form(&[("client_id", CLIENT_ID)]),
        )?;
        if result["error"].as_str() == Some("device_flow_disabled") {
            return Err(
                "Enable Device Flow in the Repo City GitHub App settings to sign in here.".into(),
            );
        }
        serde_json::from_value(result).map_err(|_| "GitHub did not issue a sign-in code.".into())
    })
}

pub(super) fn browser_sign_in() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return Command::new("open")
        .arg(DEVICE_URL)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string());
    #[cfg(not(target_os = "macos"))]
    Err(format!("Open {DEVICE_URL} in your browser."))
}

pub(super) fn finish_sign_in(code: DeviceCode) -> Job<Session> {
    use std::time::{Duration, Instant};
    Job::spawn(move || {
        let end = Instant::now() + Duration::from_secs(code.expires_in);
        let mut delay = Duration::from_secs(code.interval.max(1));
        while Instant::now() + delay < end {
            std::thread::sleep(delay);
            let result = oauth_response(
                agent()
                    .post("https://github.com/login/oauth/access_token")
                    .set("Accept", "application/json")
                    .send_form(&[
                        ("client_id", CLIENT_ID),
                        ("device_code", &code.device_code),
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ]),
            )?;
            if let Some(token) = result["access_token"].as_str() {
                let user: Value = api("user", Some(token), None)?;
                let login = user["login"]
                    .as_str()
                    .ok_or("GitHub returned no account name.")?;
                return Ok(Session {
                    token: token.into(),
                    login: login.into(),
                });
            }
            match result["error"].as_str() {
                Some("authorization_pending") => {}
                Some("slow_down") => delay += Duration::from_secs(5),
                _ => return Err("GitHub sign-in did not complete. Request a new code.".into()),
            }
        }
        Err("The GitHub sign-in code expired. Request a new code.".into())
    })
}

fn git(root: &Path, arguments: &[&str], token: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args([
        "-c",
        "credential.helper=",
        "-c",
        "http.followRedirects=false",
    ]);
    if let Some(token) = token {
        cmd.arg("--config-env=http.https://github.com/.extraHeader=REPO_CITY_GIT_HEADER")
            .env(
                "REPO_CITY_GIT_HEADER",
                format!(
                    "Authorization: Basic {}",
                    STANDARD.encode(format!("x-access-token:{token}"))
                ),
            );
    }
    let Output {
        status,
        stdout,
        stderr,
    } = cmd
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(String::from_utf8_lossy(&stderr).trim().to_owned());
    }
    String::from_utf8(stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|error| error.to_string())
}

pub(super) fn development_token() -> Option<String> {
    std::env::var("REPO_CITY_GITHUB_TOKEN")
        .ok()
        .filter(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

pub(super) fn token(session: Option<&Session>) -> Option<String> {
    session
        .map(|session| session.token.clone())
        .or_else(development_token)
}

fn sha(value: &str) -> Result<&str, String> {
    if value.len() == 40 && value.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(value)
    } else {
        Err("GitHub returned an invalid snapshot identifier.".into())
    }
}

pub(super) struct Snapshot {
    pub target: Target,
    pub repository: Repository,
}

impl Snapshot {
    fn load(
        target: Target,
        token: Option<&str>,
        revision: &str,
        base: &str,
    ) -> Result<Self, String> {
        let root = target.cache()?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        if !root.join("HEAD").exists() {
            git(&root, &["init", "--bare", "--quiet"], None)?;
        }
        let origin = format!("https://github.com/{}.git", target.slug());
        let repository = if let Some(endpoint) = target.pull_endpoint() {
            let pull: Pull = api(&endpoint, token, None)?;
            let head = sha(&pull.head.sha)?;
            let base = sha(&pull.base.sha)?;
            let reference = format!("refs/pull/{}/head", target.pull.unwrap());
            git(
                &root,
                &["fetch", "--quiet", "--no-tags", &origin, &reference, base],
                token,
            )?;
            let merge_base = git(&root, &["merge-base", base, head], None)?;
            repository_picker::load(&root, head, Some(sha(&merge_base)?))?
        } else {
            let head = fetch_revision(
                &root,
                &origin,
                &target,
                token,
                if revision.is_empty() {
                    "HEAD"
                } else {
                    revision
                },
            )?;
            let merge_base = if base.is_empty() {
                None
            } else {
                let base = fetch_revision(&root, &origin, &target, token, base)?;
                Some(git(&root, &["merge-base", &base, &head], None)?)
            };
            repository_picker::load(&root, &head, merge_base.as_deref())?
        };
        Ok(Self { target, repository })
    }
}

fn fetch_revision(
    root: &Path,
    origin: &str,
    target: &Target,
    token: Option<&str>,
    revision: &str,
) -> Result<String, String> {
    let commits: Vec<Commit> = api_query(
        &format!("repos/{}/commits", target.slug()),
        token,
        None,
        &[("sha", revision), ("per_page", "1")],
    )?;
    let commit = commits
        .first()
        .ok_or("GitHub returned no commit for this revision.")?;
    let head = sha(&commit.sha)?;
    let reference = format!("{head}:refs/repo-city/snapshots/{head}");
    git(
        root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-write-fetch-head",
            "--",
            origin,
            &reference,
        ],
        token,
    )?;
    Ok(head.to_owned())
}

pub(super) struct Job<T>(Mutex<mpsc::Receiver<Result<T, String>>>);

impl<T: Send + 'static> Job<T> {
    pub(super) fn spawn(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Self {
        let (send, receive) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = send.send(work());
        });
        Self(Mutex::new(receive))
    }

    pub fn take(&self) -> Option<Result<T, String>> {
        match self.0.lock().expect("GitHub worker receiver").try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("GitHub task ended unexpectedly.".into()))
            }
        }
    }
}

pub(super) fn open(
    target: Target,
    token: Option<String>,
    revision: String,
    base: String,
) -> Job<Snapshot> {
    Job::spawn(move || Snapshot::load(target, token.as_deref(), &revision, &base))
}

fn review_comment(note: &Note, body: String) -> Value {
    let anchor = &note.anchor;
    let side = if anchor.before { "LEFT" } else { "RIGHT" };
    let mut payload = json!({
        "body": body, "commit_id": note.revision, "path": anchor.path,
        "line": anchor.end, "side": side,
    });
    if anchor.start != anchor.end {
        payload["start_line"] = json!(anchor.start);
        payload["start_side"] = json!(side);
    }
    payload
}

pub(super) fn publish(target: Target, mut note: Note, token: String) -> Job<Note> {
    Job::spawn(move || {
        let endpoint = target
            .pull_endpoint()
            .ok_or("Open a GitHub pull request first.")?;
        let pull: Pull = api(&endpoint, Some(&token), None)?;
        if pull.state != "open" {
            return Err("This pull request is closed. Your draft is saved.".into());
        }
        if pull.head.sha != note.revision {
            return Err("The pull request changed. Reopen it and select the current lines.".into());
        }
        let comments = format!("{endpoint}/comments");
        let marker = format!("<!-- repo-city:native:{} -->", note.id);
        let mut existing = None;
        for page in 1..=12 {
            let batch: Vec<Comment> = api(
                &format!("{comments}?per_page=100&page={page}"),
                Some(&token),
                None,
            )?;
            let last = batch.len() < 100;
            existing = batch
                .into_iter()
                .find(|comment| comment.body.ends_with(&marker));
            if existing.is_some() || last {
                break;
            }
            if page == 12 {
                return Err("Check for the note on GitHub before retrying. There are too many comments to check safely.".into());
            }
        }
        let comment: Comment = if let Some(existing) = existing {
            existing
        } else {
            let text = if note.text.trim().is_empty() {
                &note.preset.prompt
            } else {
                &note.text
            };
            if text.trim().is_empty() {
                return Err("Write a comment or set a prompt in the paint preset.".into());
            }
            let payload = review_comment(&note, format!("{}\n\n{marker}", text.trim()));
            api(&comments, Some(&token), Some(&payload))?
        };
        note.published = Some(comment.html_url);
        Ok(note)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_notes::{Anchor, Preset};

    #[test]
    fn oauth_keeps_the_error_code_from_an_http_400_response() {
        let response =
            ureq::Response::new(400, "Bad Request", r#"{"error":"device_flow_disabled"}"#).unwrap();
        let result = oauth_response(Err(ureq::Error::Status(400, response))).unwrap();
        assert_eq!(result["error"], "device_flow_disabled");
    }

    #[test]
    fn github_targets_keep_pr_caches_separate_and_reject_non_github_inputs() {
        let target = Target::parse("https://github.com/Owner/Repo/pull/34/").unwrap();
        assert_eq!(
            target.pull_endpoint().as_deref(),
            Some("repos/owner/repo/pulls/34")
        );
        assert!(target.cache().unwrap().ends_with("owner/repo/pr-34.git"));
        assert_eq!(Target::parse("owner/repo").unwrap().pull, None);
        for input in [
            "https://example.com/owner/repo",
            "owner/..",
            "owner/repo/pull/0",
            "owner/repo/tree/main",
        ] {
            assert!(Target::parse(input).is_err());
        }
    }

    #[test]
    fn review_payload_uses_the_snapshot_and_both_ends_of_a_removed_range() {
        let mut note = Note {
            id: "test".into(),
            revision: "a".repeat(40),
            baseline: None,
            anchor: Anchor {
                path: "src/lib.rs".into(),
                before: true,
                start: 4,
                end: 7,
            },
            preset: Preset {
                id: "bug".into(),
                label: "Bug".into(),
                spray: String::new(),
                prompt: String::new(),
                color: [0; 3],
            },
            text: String::new(),
            published: None,
            remote: false,
            resolved: None,
            attention: false,
        };
        let payload = review_comment(&note, "Comment".into());
        assert_eq!(payload["commit_id"], note.revision);
        assert_eq!(payload["start_line"], 4);
        assert_eq!(payload["line"], 7);
        assert_eq!(payload["start_side"], "LEFT");
        note.anchor.before = false;
        note.anchor.start = 7;
        let payload = review_comment(&note, "Comment".into());
        assert_eq!(payload["side"], "RIGHT");
        assert!(payload.get("start_side").is_none());
    }
}
