use super::*;
use repo_world::Blob;

fn repo(revision: &str, paths: &[(&str, Change)]) -> Repository {
    let blob = || Blob {
        id: "blob".into(),
        size: 5,
        mode: "100644".into(),
    };
    Repository {
        root: "/repo-city-test".into(),
        revision: revision.into(),
        baseline: Some("base".into()),
        files: paths
            .iter()
            .map(|(path, change)| File {
                path: (*path).into(),
                before: (*change != Change::Added).then(blob),
                after: (*change != Change::Removed).then(blob),
                change: change.clone(),
                position: [0., 0.],
            })
            .collect(),
        districts: Vec::new(),
    }
}

fn paths(book: &Book) -> Vec<&str> {
    book.active()
        .tiles
        .iter()
        .map(|tile| tile.path.as_str())
        .collect()
}

#[test]
fn default_uses_comparison_paths_and_accepts_extra_paths_once() {
    let repository = repo(
        "head",
        &[
            ("a", Change::Modified),
            ("b", Change::Unchanged),
            ("c", Change::Removed),
        ],
    );
    let mut book = Book::new(Scope("fixture".into()), &repository);
    assert_eq!(paths(&book), ["a", "c"]);
    let removed = book.active().tiles[1].file(&repository).unwrap();
    assert!(removed.after.is_none() && removed.before.is_some());
    assert!(book.add_paths(&repository, ["b", "a", "unknown", "b"]));
    assert_eq!(paths(&book), ["a", "c", "b"]);
    assert!(!book.add_paths(&repository, ["b", "b"]));
}

#[test]
fn live_refresh_preserves_order_and_viewports_and_drops_reverted_auto_tiles() {
    let first = repo(
        "worktree:first",
        &[
            ("a", Change::Modified),
            ("b", Change::Modified),
            ("c", Change::Modified),
        ],
    );
    let next = repo(
        "worktree:next",
        &[
            ("a", Change::Unchanged),
            ("b", Change::Unchanged),
            ("c", Change::Removed),
            ("d", Change::Added),
        ],
    );
    assert_eq!(Scope::for_repository(&first), Scope::for_repository(&next));
    let mut book = Book::new(Scope("fixture".into()), &first);
    assert!(book.add_paths(&first, ["b"]));
    book.active_mut().tiles[2].viewport = Viewport { row: 40, column: 8 };
    assert!(book.refresh(&next));
    assert_eq!(paths(&book), ["b", "c", "d"]);
    assert_eq!(
        book.active().tiles[1].viewport,
        Viewport { row: 40, column: 8 }
    );
    assert!(!book.refresh(&next));
}

#[test]
fn named_canvases_restore_selection_order_and_viewports_with_scope_check() {
    let repository = repo("head", &[("a", Change::Modified), ("b", Change::Unchanged)]);
    let scope = Scope("fixture".into());
    let mut book = Book::new(scope.clone(), &repository);
    book.create(" Related files ").unwrap();
    assert!(book.create("Related files").is_err());
    assert!(book.add_paths(&repository, ["b", "a"]));
    let active = book.active_mut();
    active.zoom = 1.5;
    active.pan = [30., -25.];
    active.tiles[1].viewport.row = 88;
    active.tiles[1].positioned = true;
    active.tiles[1].size = [780., 400.];

    let encoded = book.encode().unwrap();
    let mut reopened = Book::decode(&scope, &encoded).unwrap();
    assert!(Book::decode(&Scope("other".into()), &encoded).is_err());
    assert_eq!(reopened.active().name, "Related files");
    assert_eq!(paths(&reopened), ["b", "a"]);
    assert_eq!(reopened.active().zoom, 1.5);
    assert_eq!(reopened.active().pan, [30., -25.]);
    assert_eq!(reopened.active().tiles[1].viewport.row, 88);
    assert!(reopened.active().tiles[1].positioned);
    assert_eq!(reopened.active().tiles[1].size, [780., 400.]);
    assert!(reopened.select(0));
    assert_eq!(paths(&reopened), ["a"]);
}

#[test]
fn layout_is_saved_and_worktree_refresh_never_moves_surviving_panels() {
    let scope = Scope("layout".into());
    let before = repo(
        "worktree:before",
        &[
            ("a", Change::Modified),
            ("b", Change::Modified),
            ("c", Change::Modified),
        ],
    );
    let after = repo(
        "worktree:after",
        &[
            ("a", Change::Unchanged),
            ("b", Change::Modified),
            ("c", Change::Modified),
            ("d", Change::Added),
        ],
    );
    let mut book = Book::new(scope.clone(), &before);
    assert!(book.active_mut().place_tiles(1280.));
    let rect = book.active().tiles[1].rect().unwrap();
    assert!(book.refresh(&after));
    assert!(book.active_mut().place_tiles(750.));
    assert_eq!(book.active().tiles[0].rect(), Some(rect));
    let reopened = Book::decode(&scope, &book.encode().unwrap()).unwrap();
    assert_eq!(reopened.active().tiles[0].rect(), Some(rect));
    assert!(reopened.active().tiles[2].rect().is_some());
}

#[test]
fn resizing_shares_only_the_touched_divider_and_respects_minimums() {
    let repo = repo(
        "head",
        &[
            ("a", Change::Modified),
            ("b", Change::Modified),
            ("c", Change::Modified),
        ],
    );
    let mut book = Book::new(Scope("resize".into()), &repo);
    let canvas = book.active_mut();
    assert!(canvas.place_tiles(1280.));
    let [a, b, c] = [0, 1, 2].map(|i| canvas.tiles[i].rect().unwrap());
    assert!(canvas.resize("a", Axis::Horizontal, a.size.x + 160.));
    let [left, right, below] = [0, 1, 2].map(|i| canvas.tiles[i].rect().unwrap());
    assert_eq!(left.origin, a.origin);
    assert_eq!(left.size.x, a.size.x + 160.);
    assert_eq!(right.origin.x, b.origin.x + 160.);
    assert_eq!(right.end(), b.end());
    assert_eq!(below, c);
    assert!(canvas.resize("a", Axis::Horizontal, 2400.));
    assert_eq!(canvas.tiles[1].rect().unwrap().size.x, 280.);
    assert_eq!(canvas.tiles[2].rect(), Some(c));
}
