//! Immutable repository inventory and lazy source loading. No checkout or source writes.
pub mod anatomy;
pub mod comparison;
pub mod syntax;

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
#[derive(Clone, Debug, serde::Deserialize)]
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
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output.stdout)
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
        if paths.len() > 5000 {
            return Err(format!(
                "{} files: first version supports up to 5000. Select a smaller repository.",
                paths.len()
            ));
        }
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
            let dir = f.path.rsplit_once('/').map_or("/", |(d, _)| d);
            groups.entry(dir.into()).or_default().push(i);
        }
        let mut x = 0.;
        let mut z = 0.;
        let mut band_height = 0.0_f32;
        for (n, (name, mut indices)) in groups.into_iter().enumerate() {
            if n > 0 && n % 3 == 0 {
                x = 0.;
                z += band_height + 20.;
                band_height = 0.;
            }
            let rows = indices.len().div_ceil(6);
            let width = 112.;
            let depth = rows as f32 * 20. + 16.;
            let columns = indices.len().min(6);
            let mut lots: Vec<_> = (0..indices.len()).collect();
            lots.sort_by_key(|&lot| {
                let (column, row) = (lot % 6, lot / 6);
                let side = column.min(columns - 1 - column);
                let end = row.min(rows - 1 - row);
                (side.min(end), side + end, lot)
            });
            indices.sort_by_key(|&i| {
                let file = &self.files[i];
                let size = file
                    .after
                    .as_ref()
                    .or(file.before.as_ref())
                    .map_or(0, |blob| blob.size);
                (std::cmp::Reverse(size), i)
            });
            for (j, i) in lots.into_iter().zip(indices) {
                self.files[i].position = [
                    x + 12. + (j % 6) as f32 * 17.,
                    z + 12. + (j / 6) as f32 * 20.,
                ];
            }
            self.districts.push(District {
                name,
                position: [x, z],
                size: [width, depth],
            });
            x += width + 20.;
            band_height = band_height.max(depth);
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
        if blob.size > 1024 * 1024 {
            return Err("Source exceeds the 1 MiB inspection limit".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let bytes = git(&self.root, &["cat-file", "blob", &blob.id])?;
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
                    && f.before.as_ref().is_some_and(|b| b.size <= 1024 * 1024)
                    && f.after.as_ref().is_some_and(|b| b.size <= 1024 * 1024)
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
    fn tall_files_take_perimeter_lots_before_interior_lots() {
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
            .collect();
        let mut repo = Repository {
            root: PathBuf::new(),
            revision: String::new(),
            baseline: None,
            files,
            districts: Vec::new(),
        };
        repo.layout();
        for (size, file) in repo.files.iter().enumerate() {
            let [x, z] = file.position;
            let edge = x == 12. || x == 97. || z == 12. || z == 72.;
            assert_eq!(edge, size >= 8, "lot for {}", file.path);
        }
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
