use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Selection {
    pub anchor: review_notes::Anchor,
    pub blob: String,
}

impl Selection {
    pub fn index(&self, repo: &Repository) -> Result<usize, String> {
        let index = repo
            .files
            .binary_search_by(|file| file.path.cmp(&self.anchor.path))
            .ok();
        index
            .filter(|&index| {
                let file = &repo.files[index];
                let blob = if self.anchor.before {
                    &file.before
                } else {
                    &file.after
                };
                blob.as_ref().is_some_and(|blob| blob.id == self.blob)
            })
            .ok_or_else(|| "Source changed. Select the lines again.".into())
    }
}

pub(super) enum Document {
    Comment(Selection, String),
    Edit(source_editor::DraftIdentity, String),
}

impl Document {
    pub fn edit(repo: &Repository, selection: &Selection) -> Result<Self, String> {
        let index = selection.index(repo)?;
        let anchor = &selection.anchor;
        let source = repo.source(index, anchor.before)?;
        let range = anchor.start - 1..anchor.end;
        source_editor::chunk_bytes(&source, range.clone())?;
        Ok(Self::Edit(
            source_editor::DraftIdentity {
                repository: repo.root.to_string_lossy().into_owned(),
                revision: repo.revision.clone(),
                baseline: repo.baseline.clone(),
                path: anchor.path.clone(),
                blob: selection.blob.clone(),
                before: anchor.before,
                range,
            },
            source,
        ))
    }
}
