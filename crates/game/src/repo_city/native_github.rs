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

#[derive(Deserialize)]
struct PullComment {
    id: u64,
    path: String,
    #[serde(flatten)]
    comment: Comment,
    commit_id: String,
    position: Option<usize>,
    line: Option<usize>,
    start_line: Option<usize>,
    side: Option<String>,
    start_side: Option<String>,
    original_line: Option<usize>,
    original_start_line: Option<usize>,
    in_reply_to_id: Option<u64>,
}

impl PullComment {
    fn into_note(self, revision: &str, baseline: &Option<String>) -> Option<Note> {
        use crate::review_notes::{Anchor, Preset};
        if self.in_reply_to_id.is_some() {
            return None;
        }
        let side = self.side.as_deref();
        let anchored = self.commit_id == revision
            && self.position.is_some()
            && self
                .line
                .is_some_and(|line| line > 0 && self.start_line.unwrap_or(line) <= line)
            && matches!(side, Some("LEFT" | "RIGHT"))
            && self.start_side.as_deref().or(side) == side;
        let (start, end) = if anchored {
            let end = self.line.unwrap_or_default();
            (self.start_line.unwrap_or(end), end)
        } else {
            let end = self.original_line.unwrap_or_default();
            (self.original_start_line.unwrap_or(end), end)
        };
        let text = self.comment.body;
        let text = text
            .rsplit_once("\n\n<!-- repo-city:")
            .filter(|(_, marker)| marker.ends_with(" -->") && !marker.contains('\n'))
            .map_or_else(|| text.clone(), |(body, _)| body.to_owned());
        Some(Note {
            id: format!("github-{}", self.id),
            revision: if anchored {
                revision.into()
            } else {
                String::new()
            },
            baseline: baseline.clone(),
            source_blob: None,
            anchor: Anchor {
                path: self.path,
                start,
                end,
                before: side == Some("LEFT"),
            },
            preset: Preset {
                id: "comment".into(),
                label: "GitHub".into(),
                spray: String::new(),
                prompt: String::new(),
                color: [151, 199, 201],
            },
            text,
            published: Some(self.comment.html_url),
            remote: true,
            resolved: None,
            attention: false,
        })
    }
}

#[derive(bevy::prelude::Resource)]
pub(super) enum PullComments {
    Loading,
    Loaded(Result<Vec<Note>, String>),
}

pub(super) fn pull_comments(
    target: &Target,
    repository: &Repository,
    token: Option<String>,
) -> Option<Job<Vec<Note>>> {
    let endpoint = target.pull_endpoint()?;
    let revision = repository.revision.clone();
    let baseline = repository.baseline.clone();
    Some(Job::spawn(move || {
        let mut notes = Vec::new();
        let mut page = 1;
        loop {
            let batch: Vec<PullComment> = api(
                &format!("{endpoint}/comments?per_page=100&page={page}"),
                token.as_deref(),
                None,
            )?;
            let last = batch.len() < 100;
            notes.extend(
                batch
                    .into_iter()
                    .filter_map(|item| item.into_note(&revision, &baseline)),
            );
            if last {
                return Ok(notes);
            }
            page += 1;
        }
    }))
}

#[derive(Deserialize, serde::Serialize)]
pub(super) struct Session {
    token: String,
    pub login: String,
    expires_at: Option<u64>,
}

const CLIENT_ID: &str = "Iv23liflIsPOavfEGn6b";
const DEVICE_URL: &str = "https://github.com/login/device";
#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "dev.meta-uber-engineer.repo-city.github";

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Session {
    pub fn valid(&self) -> bool {
        self.expires_at.is_none_or(|end| end > epoch_seconds())
    }

    #[cfg(target_os = "macos")]
    fn remember(&self) {
        if let Ok(data) = serde_json::to_vec(self)
            && security_framework::passwords::set_generic_password(
                KEYCHAIN_SERVICE,
                CLIENT_ID,
                &data,
            )
            .is_err()
        {
            bevy::log::warn!("Could not save the GitHub session in Keychain.");
        }
    }
}

pub(super) fn restore_session() -> Job<Option<Session>> {
    Job::spawn(|| {
        #[cfg(target_os = "macos")]
        {
            use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
            let result = ItemSearchOptions::new()
                .class(ItemClass::generic_password())
                .service(KEYCHAIN_SERVICE)
                .account(CLIENT_ID)
                .load_data(true)
                .skip_authenticated_items(true)
                .search();
            if let Ok(items) = result {
                for item in items {
                    if let SearchResult::Data(data) = item
                        && let Ok(session) = serde_json::from_slice::<Session>(&data)
                        && session.valid()
                    {
                        return Ok(Some(session));
                    }
                }
            }
        }
        Ok(None)
    })
}

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
    open_browser_url(DEVICE_URL)
}

pub(super) fn open_discussion(url: &str) -> Result<(), String> {
    if !url.starts_with("https://github.com/") {
        return Err("This discussion has no GitHub link.".into());
    }
    open_browser_url(url)
}

pub(super) fn open_browser_url(url: &str) -> Result<(), String> {
    browser_command(std::env::consts::OS, url)?
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open your browser: {error}. Open {url} manually."))
}

fn browser_command(platform: &str, url: &str) -> Result<Command, String> {
    let mut command = match platform {
        "macos" => Command::new("open"),
        "linux" => Command::new("xdg-open"),
        "windows" => {
            let mut command = Command::new("powershell.exe");
            // Pass the URL as data, never as executable PowerShell source.
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Process -FilePath $env:VIBE_CITY_BROWSER_URL",
            ]);
            command.env("VIBE_CITY_BROWSER_URL", url);
            return Ok(command);
        }
        _ => return Err(format!("Open {url} in your browser.")),
    };
    command.arg(url);
    Ok(command)
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
                let session = Session {
                    token: token.into(),
                    login: login.into(),
                    expires_at: result["expires_in"]
                        .as_u64()
                        .map(|seconds| epoch_seconds().saturating_add(seconds)),
                };
                #[cfg(target_os = "macos")]
                session.remember();
                return Ok(session);
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
        .filter(|session| session.valid())
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

/// Room comparisons use exact object IDs. Normal branch selection may resolve a
/// merge base; this path must preserve the room's pinned base and fork identity.
pub(super) fn open_pinned(
    target: Target,
    token: Option<String>,
    revision: String,
    base: String,
    head_repository: Option<Target>,
) -> Job<Snapshot> {
    Job::spawn(move || {
        sha(&revision)?;
        sha(&base)?;
        let root = target.cache()?;
        std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        if !root.join("HEAD").exists() {
            git(&root, &["init", "--bare", "--quiet"], None)?;
        }
        let head_target = head_repository.as_ref().unwrap_or(&target);
        let head = fetch_revision(
            &root,
            &format!("https://github.com/{}.git", head_target.slug()),
            head_target,
            token.as_deref(),
            &revision,
        )?;
        let baseline = fetch_revision(
            &root,
            &format!("https://github.com/{}.git", target.slug()),
            &target,
            token.as_deref(),
            &base,
        )?;
        if head != revision || baseline != base {
            return Err("The room's pinned comparison is unavailable.".into());
        }
        let repository = repository_picker::load(&root, &head, Some(&baseline))?;
        Ok(Snapshot { target, repository })
    })
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

pub(super) fn publish(
    target: Target,
    notes: Vec<Note>,
    token: String,
) -> Job<Vec<Result<Note, String>>> {
    Job::spawn(move || {
        let endpoint = target
            .pull_endpoint()
            .ok_or("Open a GitHub pull request first.")?;
        let pull: Pull = api(&endpoint, Some(&token), None)?;
        if notes.iter().any(|note| pull.head.sha != note.revision) {
            return Err("The pull request changed. Reopen it and select the current lines.".into());
        }
        let comments = format!("{endpoint}/comments");
        let mut existing = Vec::<Comment>::new();
        for page in 1..=12 {
            let batch: Vec<Comment> = api(
                &format!("{comments}?per_page=100&page={page}"),
                Some(&token),
                None,
            )?;
            let last = batch.len() < 100;
            existing.extend(batch);
            if last {
                break;
            }
            if page == 12 {
                return Err("Check for the notes on GitHub before retrying. There are too many comments to check safely.".into());
            }
        }
        Ok(notes
            .into_iter()
            .map(|mut note| {
                let marker = format!("<!-- repo-city:native:{} -->", note.id);
                note.published = if let Some(comment) = existing
                    .iter()
                    .find(|comment| comment.body.ends_with(&marker))
                {
                    Some(comment.html_url.clone())
                } else {
                    let text = note.comment().trim();
                    if text.is_empty() {
                        return Err(format!(
                            "{}:{} has no comment text.",
                            note.anchor.path, note.anchor.start
                        ));
                    }
                    let payload = review_comment(&note, format!("{text}\n\n{marker}"));
                    let comment: Comment = api(&comments, Some(&token), Some(&payload))?;
                    Some(comment.html_url)
                };
                Ok(note)
            })
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_launcher_does_not_interpret_the_url_as_powershell() {
        let url = "https://github.com/login/device?value=';Write-Output+unexpected";
        let command = browser_command("windows", url).unwrap();
        let arguments: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(command.get_program(), "powershell.exe");
        assert!(!arguments.iter().any(|arg| arg.contains(url)));
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "VIBE_CITY_BROWSER_URL"
                    && value == Some(std::ffi::OsStr::new(url)))
        );
        let linux = browser_command("linux", url).unwrap();
        assert_eq!(linux.get_program(), "xdg-open");
        assert_eq!(linux.get_args().next(), Some(std::ffi::OsStr::new(url)));
    }
    use crate::review_notes::{Anchor, Preset};

    #[test]
    fn merged_pull_comments_mark_current_lines_and_keep_outdated_discussions() {
        let revision = "head";
        let base = Some("base".to_string());
        let stored = serde_json::json!({
            "id": 1, "path": "src/lib.rs", "body": "Explain this.", "html_url": "https://github.com/a/b/pull/3#discussion_r1",
            "line": 9, "start_line": 7, "side": "RIGHT", "start_side": "RIGHT",
            "position": 3, "commit_id": revision, "original_line": 10
        });
        let current = serde_json::from_value::<PullComment>(stored.clone())
            .unwrap()
            .into_note(revision, &base)
            .unwrap();
        assert_eq!((current.anchor.start, current.anchor.end), (7, 9));
        assert_eq!(current.revision, revision);
        assert!(current.remote);
        let mut outdated = stored;
        outdated["position"] = Value::Null;
        let original = serde_json::from_value::<PullComment>(outdated.clone())
            .unwrap()
            .into_note(revision, &base)
            .unwrap();
        assert_ne!(original.revision, revision);
        assert_eq!(original.anchor.end, 10);
        outdated["in_reply_to_id"] = json!(1);
        assert!(
            serde_json::from_value::<PullComment>(outdated)
                .unwrap()
                .into_note(revision, &base)
                .is_none()
        );
    }

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
            source_blob: None,
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
