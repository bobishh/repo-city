//! Immutable repository inventory and lazy source loading. No checkout or source writes.
#[cfg(not(target_arch = "wasm32"))]
pub mod blame;
#[cfg(not(target_arch = "wasm32"))]
pub mod history;

pub mod anatomy;
pub mod comparison;
#[cfg(not(target_arch = "wasm32"))]
pub mod links;
pub mod syntax;
#[cfg(not(target_arch = "wasm32"))]
mod worktree;

pub use anatomy::{CallableAnatomy, FileAnatomy};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Change {
    Added,
    Removed,
    Modified,
    Unchanged,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Blob {
    pub id: String,
    pub size: usize,
    pub mode: String,
}
#[derive(Clone, Debug)]
pub struct File {
    pub path: String,
    pub before: Option<Blob>,
    pub after: Option<Blob>,
    pub change: Change,
    pub position: [f32; 2],
}

impl File {
    pub fn directory(&self) -> &str {
        self.path
            .rsplit_once('/')
            .map_or("/", |(directory, _)| directory)
    }
}
#[derive(Clone, Debug)]
pub struct District {
    pub name: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct Repository {
    pub root: PathBuf,
    pub revision: String,
    pub baseline: Option<String>,
    pub files: Vec<File>,
    pub districts: Vec<District>,
}
/// Compressed binary preview data.
pub const ASSET_BYTES_LIMIT: usize = 16 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
fn asset_blob(root: &Path, id: &str) -> Result<Vec<u8>, String> {
    use std::{io::Read, process::Stdio};
    if !matches!(id.len(), 40 | 64) || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid asset blob ID.".into());
    }
    let mut command = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "blob", id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let read = command
        .stdout
        .take()
        .ok_or("Asset reader is unavailable.")?
        .take((ASSET_BYTES_LIMIT + 1) as u64)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > ASSET_BYTES_LIMIT {
        let _ = command.kill();
        let _ = command.wait();
        return Err(read.err().map_or_else(
            || "Asset exceeds the 16 MiB preview limit.".into(),
            |error| error.to_string(),
        ));
    }
    let output = command
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(bytes)
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                history::command_error(error)
            }
            #[cfg(target_arch = "wasm32")]
            {
                error.to_string()
            }
        })?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output.stdout)
}
/// Unix commit times for paths touched in the comparison's history.
#[cfg(not(target_arch = "wasm32"))]
pub fn changed_path_times(
    root: &Path,
    baseline: Option<&str>,
    revision: &str,
) -> Result<std::collections::HashMap<String, u64>, String> {
    let range = baseline.map_or_else(|| revision.to_owned(), |base| format!("{base}..{revision}"));
    let log = git(
        root,
        &[
            "log",
            "--format=%x00%ct",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-merges=first-parent",
            &range,
            "--",
        ],
    )?;
    Ok(path_times(&log))
}

#[cfg(not(target_arch = "wasm32"))]
fn path_times(log: &[u8]) -> std::collections::HashMap<String, u64> {
    let mut times = std::collections::HashMap::new();
    let mut fields = log.split(|&byte| byte == 0);
    let mut commit = None;
    let mut first_path = false;
    while let Some(field) = fields.next() {
        if field.is_empty() {
            commit = fields
                .next()
                .and_then(|time| std::str::from_utf8(time).ok()?.parse::<u64>().ok());
            first_path = true;
        } else if let Some(time) = commit {
            let path = if std::mem::take(&mut first_path) {
                field.strip_prefix(b"\n").unwrap_or(field)
            } else {
                field
            };
            if let Ok(path) = std::str::from_utf8(path) {
                times
                    .entry(path.to_owned())
                    .and_modify(|latest: &mut u64| *latest = (*latest).max(time))
                    .or_insert(time);
            }
        }
    }
    times
}

fn resolve_revision(root: &Path, rev: &str) -> Result<String, String> {
    let data = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )?;
    let hash = String::from_utf8_lossy(&data).trim().to_owned();
    if !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid commit ID".into());
    }
    Ok(hash)
}
#[cfg(not(target_arch = "wasm32"))]
pub fn local_revisions(root: &Path) -> Result<Vec<(String, String)>, String> {
    let refs = git(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--count=150",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let commits = git(
        root,
        &[
            "log",
            "--all",
            "-n",
            "100",
            "--format=%H%x09%s",
            "HEAD",
            "--",
        ],
    )?;
    let mut choices = vec![("HEAD".into(), "HEAD · Current checkout".into())];
    for reference in String::from_utf8_lossy(&refs).lines() {
        if reference.starts_with("refs/remotes/") && reference.ends_with("/HEAD") {
            continue;
        }
        if let Some((kind, name)) = reference
            .strip_prefix("refs/")
            .and_then(|path| path.split_once('/'))
        {
            let prefix = match kind {
                "heads" => "Branch",
                "remotes" => "Remote branch",
                _ => "Tag",
            };
            choices.push((reference.into(), format!("{prefix} · {name}")));
        }
    }
    for line in String::from_utf8_lossy(&commits).lines() {
        if let Some((sha, message)) = line.split_once('\t')
            && let Some(short) = sha.get(..10)
        {
            choices.push((
                sha.into(),
                format!("{short} · {}", message.chars().take(85).collect::<String>()),
            ));
        }
    }
    Ok(choices)
}

fn tree(root: &Path, rev: &str) -> Result<BTreeMap<String, Blob>, String> {
    let output = git(root, &["ls-tree", "-r", "-l", "-z", rev])?;
    let mut files = BTreeMap::new();
    for entry in output.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let Some(tab) = entry.iter().position(|b| *b == b'\t') else {
            return Err("Malformed Git tree".into());
        };
        let header = std::str::from_utf8(&entry[..tab]).map_err(|e| e.to_string())?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 4 || fields[1] != "blob" {
            continue;
        }
        let name = String::from_utf8(entry[tab + 1..].to_vec())
            .map_err(|_| "Non-UTF-8 path is not supported")?;
        files.insert(
            name,
            Blob {
                id: fields[2].into(),
                size: fields[3].parse().map_err(|_| "Invalid blob size")?,
                mode: fields[0].into(),
            },
        );
    }
    Ok(files)
}
impl Repository {
    pub fn is_worktree(&self) -> bool {
        self.revision.starts_with("worktree:")
    }

    pub fn open(root: &Path, rev: &str) -> Result<Self, String> {
        Self::open_with_base(root, rev, None)
    }
    pub fn open_with_base(root: &Path, rev: &str, base: Option<&str>) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let revision = resolve_revision(&root, rev)?;
        let baseline = if let Some(base) = base {
            Some(resolve_revision(&root, base)?)
        } else {
            let parents = git(&root, &["rev-list", "--parents", "-n", "1", &revision])?;
            String::from_utf8_lossy(&parents)
                .split_whitespace()
                .nth(1)
                .map(str::to_owned)
        };
        let after = tree(&root, &revision)?;
        let before = baseline
            .as_ref()
            .map(|r| tree(&root, r))
            .transpose()?
            .unwrap_or_default();
        Self::from_trees(root, revision, baseline, before, after)
    }

    fn from_trees(
        root: PathBuf,
        revision: String,
        baseline: Option<String>,
        before: BTreeMap<String, Blob>,
        after: BTreeMap<String, Blob>,
    ) -> Result<Self, String> {
        let mut paths: Vec<_> = before.keys().chain(after.keys()).cloned().collect();
        paths.sort();
        paths.dedup();
        let files = paths
            .into_iter()
            .map(|path| {
                let a = after.get(&path).cloned();
                let b = before.get(&path).cloned();
                let change = match (&b, &a) {
                    (None, Some(_)) => Change::Added,
                    (Some(_), None) => Change::Removed,
                    (Some(b), Some(a)) if a.id != b.id || a.mode != b.mode => Change::Modified,
                    _ => Change::Unchanged,
                };
                File {
                    path,
                    before: b,
                    after: a,
                    change,
                    position: [0., 0.],
                }
            })
            .collect();
        let mut repo = Self {
            root,
            revision,
            baseline,
            files,
            districts: Vec::new(),
        };
        repo.layout();
        Ok(repo)
    }
    fn layout(&mut self) {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, f) in self.files.iter().enumerate() {
            groups.entry(f.directory().into()).or_default().push(i);
        }
        const LOT_WIDTH: f32 = 27.;
        const LOT_DEPTH: f32 = 30.;
        const BORDER: f32 = 4.;
        let plans = groups
            .into_iter()
            .map(|(name, mut files)| {
                let columns = (files.len() as f32 * LOT_DEPTH / LOT_WIDTH)
                    .sqrt()
                    .round()
                    .max(1.) as usize;
                let rows = files.len().div_ceil(columns);
                let blob_size = |index: usize| {
                    let file = &self.files[index];
                    file.after
                        .as_ref()
                        .or(file.before.as_ref())
                        .map_or(0, |blob| blob.size)
                };
                // From the front street, lower roofs lead to taller rows behind them.
                files.sort_by_key(|&index| (std::cmp::Reverse(blob_size(index)), index));
                let size = [
                    columns as f32 * LOT_WIDTH + 2. * BORDER,
                    rows as f32 * LOT_DEPTH + 2. * BORDER,
                ];
                (name, files, columns, size)
            })
            .collect::<Vec<_>>();
        let boxes = plans
            .iter()
            .map(|(_, _, _, size)| *size)
            .collect::<Vec<_>>();
        let positions = pack_districts(&boxes);
        self.districts.clear();
        for ((name, files, columns, size), [x, z]) in plans.into_iter().zip(positions) {
            for (row, slots) in files.chunks(columns).enumerate() {
                let offset = (columns - slots.len()) as f32 * LOT_WIDTH * 0.5;
                for (column, &index) in slots.iter().enumerate() {
                    self.files[index].position = [
                        x + BORDER + offset + (column as f32 + 0.5) * LOT_WIDTH,
                        z + BORDER + (row as f32 + 0.5) * LOT_DEPTH,
                    ];
                }
            }
            self.districts.push(District {
                name,
                position: [x, z],
                size,
            });
        }
    }
    pub fn source(&self, index: usize, before: bool) -> Result<String, String> {
        let f = self.files.get(index).ok_or("Unknown file")?;
        let blob = if before { &f.before } else { &f.after }
            .as_ref()
            .ok_or("File does not exist in this snapshot")?;
        if blob.mode == "120000" {
            return Err("Symbolic link: source is not followed".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = if blob.id.starts_with("worktree:") {
            worktree::snapshot_bytes(&self.root, &f.path, blob)?
        } else {
            git(&self.root, &["cat-file", "blob", &blob.id])?
        };
        #[cfg(target_arch = "wasm32")]
        let bytes = browser_blob(&blob.id).map_err(|error| {
            error
                .as_string()
                .unwrap_or_else(|| "Source unavailable".into())
        })?;
        if bytes.contains(&0) {
            return Err("Binary asset: metadata only".into());
        }
        String::from_utf8(bytes).map_err(|_| "Non-UTF-8 asset: metadata only".into())
    }
    /// Read binary preview data from the selected snapshot without following symlinks.
    pub fn asset_bytes(&self, index: usize, before: bool) -> Result<Vec<u8>, String> {
        let file = self.files.get(index).ok_or("Unknown file")?;
        let blob = if before { &file.before } else { &file.after }
            .as_ref()
            .ok_or("File does not exist in this snapshot")?;
        if blob.mode == "120000" {
            return Err("Symbolic link: asset is not followed.".into());
        }
        if blob.size > ASSET_BYTES_LIMIT {
            return Err("Asset exceeds the 16 MiB preview limit.".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = if blob.id.starts_with("worktree:") {
            worktree::snapshot_bytes(&self.root, &file.path, blob)?
        } else {
            asset_blob(&self.root, &blob.id)?
        };
        #[cfg(target_arch = "wasm32")]
        let bytes = browser_blob(&blob.id).map_err(|error| {
            error
                .as_string()
                .unwrap_or_else(|| "Asset unavailable.".into())
        })?;
        if bytes.len() != blob.size {
            return Err("Asset size does not match the selected snapshot.".into());
        }
        Ok(bytes)
    }
    pub fn anatomy(&self, index: usize, before: bool) -> Result<(String, FileAnatomy), String> {
        let source = self.source(index, before)?;
        let anatomy = anatomy::analyze(&self.files[index].path, &source);
        Ok((source, anatomy))
    }
    pub fn initial_file(&self) -> usize {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                f.change == Change::Modified
                    && [".rs", ".js", ".ts", ".rb", ".ex", ".elm"]
                        .iter()
                        .any(|ext| f.path.ends_with(ext))
                    && f.before.is_some()
                    && f.after.is_some()
            })
            .min_by_key(|(_, f)| {
                (
                    !f.path.split('/').any(|part| part == "src"),
                    f.after.as_ref().map_or(usize::MAX, |b| b.size),
                )
            })
            .map(|(index, _)| index)
            .or_else(|| {
                self.files
                    .iter()
                    .position(|f| f.change != Change::Unchanged && f.path.ends_with(".rs"))
            })
            .or_else(|| self.files.iter().position(|f| f.path.ends_with(".rs")))
            .unwrap_or(0)
    }
}

// Pack rectangles on shelves from largest to smallest depth. Evaluate several
// widths to keep the full plan compact even when folder sizes vary widely.
fn pack_districts(boxes: &[[f32; 2]]) -> Vec<[f32; 2]> {
    const STREET: f32 = 17.;
    let mut order = (0..boxes.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| {
        boxes[b][1]
            .total_cmp(&boxes[a][1])
            .then_with(|| boxes[b][0].total_cmp(&boxes[a][0]))
            .then_with(|| a.cmp(&b))
    });
    let area = boxes
        .iter()
        .map(|[width, depth]| (width + STREET) * (depth + STREET))
        .sum::<f32>();
    let widest = boxes.iter().map(|size| size[0]).fold(0., f32::max);
    let mut best = (f32::INFINITY, vec![]);
    for factor in [0.82, 0.96, 1.10, 1.24, 1.40, 1.58] {
        let limit = (area.sqrt() * factor).max(widest);
        let (mut x, mut z, mut height, mut extent) = (0., 0., 0., 0.0_f32);
        let mut slots = vec![[0., 0.]; boxes.len()];
        for &index in &order {
            let [width, depth] = boxes[index];
            if x > 0. && x + width > limit {
                x = 0.;
                z += height + STREET;
                height = 0.;
            }
            slots[index] = [x, z];
            extent = extent.max(x + width);
            x += width + STREET;
            height = height.max(depth);
        }
        let depth = z + height;
        let cost = extent.max(depth) + (extent * depth).sqrt() * 0.30;
        if cost < best.0 {
            best = (cost, slots);
        }
    }
    best.1
}

/// Remove a declaration from a private in-memory draft. Source files remain untouched.
pub fn cut_lines(source: &str, start: u32, end: u32) -> String {
    source
        .split_inclusive('\n')
        .enumerate()
        .filter(|(i, _)| {
            let n = *i as u32 + 1;
            n < start || n > end
        })
        .map(|(_, s)| s)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_changes_keep_the_latest_commit_and_exact_paths() {
        let times = path_times(
            b"\x0060\0\nnew.rs\0with space.rs\0\x0019\0\nold.rs\0new.rs\0\x0078\0\n\nline.rs\0",
        );
        assert_eq!(times["new.rs"], 60);
        assert_eq!(times["with space.rs"], 60);
        assert_eq!(times["old.rs"], 19);
        assert_eq!(times["\nline.rs"], 78);
    }

    #[test]
    fn comparison_keeps_all_paths_above_the_city_residency_limit() {
        let after = (0..5001)
            .map(|index| {
                (
                    format!("src/{index:04}.rs"),
                    Blob {
                        id: index.to_string(),
                        mode: "100644".into(),
                        size: 40,
                    },
                )
            })
            .collect();
        let repo =
            Repository::from_trees(PathBuf::new(), "head".into(), None, BTreeMap::new(), after)
                .unwrap();
        assert_eq!(repo.files.len(), 5001);
        assert_eq!(repo.files[5000].path, "src/5000.rs");
    }

    #[test]
    fn lower_roofs_face_the_street_and_no_tall_ring_blocks_them() {
        let files = (0..24)
            .map(|size| File {
                path: format!("src/{size:02}.rs"),
                before: None,
                after: Some(Blob {
                    id: size.to_string(),
                    size,
                    mode: "100644".into(),
                }),
                change: Change::Added,
                position: [0., 0.],
            })
            .collect::<Vec<_>>();
        let mut repo = Repository {
            root: PathBuf::new(),
            revision: String::new(),
            baseline: None,
            files,
            districts: vec![],
        };
        repo.layout();
        for (size, file) in repo.files.iter().enumerate() {
            assert!(
                repo.files[size + 1..]
                    .iter()
                    .all(|larger| larger.position[1] <= file.position[1])
            );
        }
        assert!(repo.files[0].position[1] > repo.files[23].position[1]);
    }

    #[test]
    fn dozens_of_varied_districts_pack_compactly_with_streets_between_them() {
        let boxes = (0..62)
            .map(|i| {
                if i % 13 == 0 {
                    [155., 158.]
                } else if i % 3 == 0 {
                    [62., 68.]
                } else {
                    [35., 38.]
                }
            })
            .collect::<Vec<_>>();
        let slots = pack_districts(&boxes);
        let mut extents: [f32; 2] = [0., 0.];
        for (index, (&[x, z], &[width, depth])) in slots.iter().zip(&boxes).enumerate() {
            extents[0] = extents[0].max(x + width);
            extents[1] = extents[1].max(z + depth);
            for (&[other_x, other_z], &[other_width, other_depth]) in
                slots[index + 1..].iter().zip(&boxes[index + 1..])
            {
                assert!(
                    x + width + 17. <= other_x
                        || other_x + other_width + 17. <= x
                        || z + depth + 17. <= other_z
                        || other_z + other_depth + 17. <= z
                );
            }
        }
        assert!(
            extents[0].max(extents[1]) / extents[0].min(extents[1]) < 1.38,
            "{extents:?}"
        );
        assert_eq!(slots, pack_districts(&boxes));
    }

    #[test]
    fn cutting_preserves_other_source_and_can_break_syntax() {
        let source = "fn keep() {}\nfn cut() {\n  panic!();\n}\nfn also_keep() {}\n";
        let changed = cut_lines(source, 2, 4);
        assert_eq!(changed, "fn keep() {}\nfn also_keep() {}\n");
        assert_eq!(anatomy::analyze("test.rs", source).callables.len(), 3);
        assert_eq!(anatomy::analyze("test.rs", &changed).callables.len(), 2);
    }
    #[test]
    fn inventory_is_deterministic_and_lazy_at_an_immutable_revision() {
        let dir = std::env::temp_dir().join(format!("repo-city-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]).unwrap();
        std::fs::write(dir.join("main.rs"), "fn first() {}\n").unwrap();
        git(&dir, &["add", "main.rs"]).unwrap();
        git(
            &dir,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "one",
            ],
        )
        .unwrap();
        let one = Repository::open(&dir, "HEAD").unwrap();
        std::fs::write(dir.join("main.rs"), "fn second() {}\n").unwrap();
        assert_eq!(one.source(0, false).unwrap(), "fn first() {}\n");
        git(&dir, &["add", "main.rs"]).unwrap();
        git(
            &dir,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "two",
            ],
        )
        .unwrap();
        let two = Repository::open(&dir, "HEAD").unwrap();
        assert_eq!(two.files[0].change, Change::Modified);
        assert_eq!(two.source(0, true).unwrap(), one.source(0, false).unwrap());
        assert_eq!(two.files[0].position, one.files[0].position);
        git(&dir, &["branch", "review/old", &one.revision]).unwrap();
        git(&dir, &["tag", "preview", &one.revision]).unwrap();
        let choices = local_revisions(&dir).unwrap();
        assert!(
            choices
                .iter()
                .any(|(value, _)| value == "refs/heads/review/old")
        );
        assert!(
            choices
                .iter()
                .any(|(value, _)| value == "refs/tags/preview")
        );
        let selected = choices
            .iter()
            .find(|(value, label)| value == &one.revision && label.ends_with(" · one"))
            .unwrap();
        assert_eq!(
            Repository::open(&dir, &selected.0)
                .unwrap()
                .source(0, false)
                .unwrap(),
            "fn first() {}\n"
        );
        let mut review = two.clone();
        let mut added = two.files[0].clone();
        added.path = "src/lib/sceneRuntime.ts".into();
        added.before = None;
        added.change = Change::Added;
        review.files.insert(0, added);
        assert_eq!(review.files[review.initial_file()].path, "main.rs");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = repoCity, js_name = blob)]
    fn browser_blob(sha: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue>;
    #[wasm_bindgen(js_namespace = repoCity, js_name = snapshot)]
    fn browser_snapshot() -> String;
    #[wasm_bindgen(js_namespace = repoCity, js_name = showBlame)]
    fn browser_blame_link(path: &str, before: bool, line: usize);
    #[wasm_bindgen(js_namespace = repoCity, js_name = repositoryVersion)]
    pub fn browser_repository_version() -> u32;
    #[wasm_bindgen(js_namespace = repoCity, js_name = syntaxVersion)]
    pub fn browser_syntax_version() -> u32;
    #[wasm_bindgen(js_namespace = repoCity, js_name = syntaxSpans)]
    fn browser_syntax_spans(path: &str, source: &str) -> Vec<u32>;
    #[wasm_bindgen(js_namespace = repoCity, js_name = sourceVersion)]
    pub fn browser_source_version() -> u32;
}

#[cfg(target_arch = "wasm32")]
pub fn open_browser() -> Result<Repository, String> {
    #[derive(serde::Deserialize)]
    struct Snapshot {
        name: String,
        revision: String,
        baseline: Option<String>,
        before: BTreeMap<String, Blob>,
        after: BTreeMap<String, Blob>,
    }
    let snapshot: Snapshot =
        serde_json::from_str(&browser_snapshot()).map_err(|error| error.to_string())?;
    Repository::from_trees(
        snapshot.name.into(),
        snapshot.revision,
        snapshot.baseline,
        snapshot.before,
        snapshot.after,
    )
}

#[cfg(target_arch = "wasm32")]
pub fn browser_show_blame(path: &str, before: bool, line: usize) {
    browser_blame_link(path, before, line);
}
