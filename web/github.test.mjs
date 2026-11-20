import assert from "node:assert/strict";
import { test } from "node:test";
import { loadGitHub, loadPullRequests, parseRepository, searchCommits } from "./github.mjs";

test("GitHub URLs are normalized to owner and repository", () => {
  assert.equal(parseRepository("https://github.com/owner/project.git"), "owner/project");
  assert.throws(() => parseRepository("https://example.com/owner/project"));
});

test("a branch comparison inventories its merge base and loads source on demand", async () => {
  const seen = [];
  const repo = await loadGitHub("owner/project", "feature/branch", "main", async url => {
    const path = new URL(url).pathname.replace("/repos/owner/project", "");
    seen.push(path);
    if (path.startsWith("/commits/")) return Response.json({ sha: "head", parents: [{ sha: "parent" }] });
    if (path.startsWith("/compare/")) return Response.json({ merge_base_commit: { sha: "base" } });
    if (path.startsWith("/git/trees/")) return Response.json({ tree: [{ path: "src/lib.rs", type: "blob", mode: "100644", sha: path.split("/").at(-1), size: 3 }] });
    return Response.json({ encoding: "base64", content: "eHl6", size: 3 });
  });
  assert.equal(JSON.parse(repo.snapshot).baseline, "base");
  assert.deepEqual(seen, ["/commits/feature%2Fbranch", "/compare/main...head", "/git/trees/base", "/git/trees/head"]);
  assert.equal(new TextDecoder().decode(await repo.blob("head")), "xyz");
});

test("a truncated recursive tree is completed through its subtrees", async () => {
  const files = Array.from({ length: 5001 }, (_, index) => ({
    path: `file-${index}.rs`, sha: `blob-${index}`, type: "blob", mode: "100644", size: 3,
  }));
  const repo = await loadGitHub("owner/project", "main", "", async input => {
    const url = new URL(input);
    if (url.pathname.includes("/commits/")) return Response.json({ sha: "head", parents: [] });
    if (url.searchParams.has("recursive")) return Response.json({ truncated: true, tree: [] });
    if (url.pathname.endsWith("/head")) return Response.json({ tree: [
      { path: "src", sha: "folder", type: "tree" },
      { path: "tests", sha: "folder", type: "tree" },
    ] });
    assert.ok(url.pathname.endsWith("/folder"));
    return Response.json({ tree: files });
  });
  const { after } = JSON.parse(repo.snapshot);
  assert.equal(Object.keys(after).length, 10002);
  assert.equal(after["tests/file-5000.rs"].id, "blob-5000");
});

test("an individually truncated directory cannot silently drop files", async () => {
  await assert.rejects(loadGitHub("owner/project", "main", "", async url =>
    Response.json(url.includes("/commits/") ? { sha: "head", parents: [] } : { truncated: true })), /truncated/);
});

test("a fork pull request opens the full head, compares from its merge base and reads each side from the right repository", async () => {
  const seen = [];
  const loaded = await loadGitHub("https://github.com/owner/project/pull/17/files#diff", "", "", async url => {
    const path = new URL(url).pathname;
    seen.push(path);
    if (path === "/repos/owner/project/pulls/17") return Response.json({
      state: "open",
      base: { sha: "base-tip" },
      head: { sha: "head-tip", repo: { full_name: "contributor/project" } },
    });
    if (path === "/repos/owner/project/compare/base-tip...head-tip") return Response.json({ merge_base_commit: { sha: "common" } });
    if (path.includes("/git/trees/")) return Response.json({ tree: [
      { path: "src/changed.rs", type: "blob", mode: "100644", sha: path.endsWith("/common") ? "before-blob" : "after-blob", size: 3 },
      { path: "src/unchanged.rs", type: "blob", mode: "100644", sha: "shared-blob", size: 3 },
    ] });
    return Response.json({ encoding: "base64", content: "eHl6", size: 3 });
  });
  const snapshot = JSON.parse(loaded.snapshot);
  assert.equal(snapshot.name, "owner/project#17");
  assert.equal(snapshot.baseline, "common");
  assert.equal(snapshot.after["src/unchanged.rs"].id, "shared-blob");
  assert.equal(loaded.blameURL("src/changed.rs", false, 12),
    "https://github.com/contributor/project/blame/head-tip/src/changed.rs#L12");
  assert.equal(loaded.blameURL("src/changed.rs", true, 9),
    "https://github.com/owner/project/blame/common/src/changed.rs#L9");
  assert.equal(loaded.blameURL("missing.rs", false, 1), null);
  await loaded.blob("before-blob");
  await loaded.blob("after-blob");
  assert.deepEqual(seen, [
    "/repos/owner/project/pulls/17",
    "/repos/owner/project/compare/base-tip...head-tip",
    "/repos/owner/project/git/trees/common",
    "/repos/contributor/project/git/trees/head-tip",
    "/repos/owner/project/git/blobs/before-blob",
    "/repos/contributor/project/git/blobs/after-blob",
  ]);
});

test("closed and merged PRs open the historical head and merge base", async () => {
  for (const merged of [false, true]) {
    const loaded = await loadGitHub("https://github.com/owner/project/pull/17", "", "", async url => {
      const path = new URL(url).pathname;
      if (path.endsWith("/pulls/17")) return Response.json({
        state: "closed", merged, base: { sha: "before" }, head: { sha: "head", repo: null },
      });
      if (path.endsWith("/compare/before...head")) return Response.json({ merge_base_commit: { sha: "base" } });
      return Response.json({ tree: [{
        path: "src/lib.rs", type: "blob", mode: "100644", sha: path.split("/").at(-1), size: 3,
      }] });
    });
    assert.equal(loaded.review.pullState, merged ? "merged" : "closed");
    const snapshot = JSON.parse(loaded.snapshot);
    assert.equal(snapshot.before["src/lib.rs"].id, "base");
    assert.equal(snapshot.after["src/lib.rs"].id, "head");
  }
});

test("a party keeps a fork comparison pinned when the PR head moves", async () => {
  const seen = [];
  const loaded = await loadGitHub("https://github.com/owner/project/pull/17", "", "", async url => {
    const path = new URL(url).pathname; seen.push(path);
    if (path.endsWith("/pulls/17")) return Response.json({ state: "open", base: { sha: "new-base" }, head: { sha: "new-head", repo: { full_name: "contributor/project" } } });
    if (path.endsWith("/commits/pinned-head")) return Response.json({ sha: "pinned-head" });
    if (path.endsWith("/commits/pinned-base")) return Response.json({ sha: "pinned-base" });
    return Response.json({ tree: [{ path: "a.rs", type: "blob", mode: "100644", sha: "blob", size: 1 }] });
  }, { pinnedComparison: { head_sha: "pinned-head", base_sha: "pinned-base", head_owner: "contributor", head_repo: "project" } });
  assert.equal(loaded.review.revision, "pinned-head");
  assert.equal(loaded.review.baseline, "pinned-base");
  assert.ok(seen.includes("/repos/contributor/project/git/trees/pinned-head"));
  assert.ok(!seen.some(path => path.includes("/compare/")));
});

test("the PR picker fetches and labels both active and historical pull requests", async () => {
  const entries = await loadPullRequests("owner/project", async url => {
    assert.equal(new URL(url).searchParams.get("state"), "all");
    return Response.json([
      { number: 3, state: "open", title: "Current" },
      { number: 2, state: "closed", merged_at: "2026-11-16T12:00:00Z", title: "Shipped" },
      { number: 1, state: "closed", title: "Abandoned" },
    ]);
  });
  assert.match(entries[0].label, /Open/);
  assert.match(entries[1].label, /Merged/);
  assert.match(entries[2].label, /Closed/);
  assert.equal(entries[2].value, "https://github.com/owner/project/pull/1");
});

test("revision suggestions preserve qualified names and label commits with subjects", async () => {
  const { loadRevisions } = await import("./github.mjs");
  const fixture = new Map([
    ["branches", [{ name: "feature/flight" }]],
    ["tags", [{ name: "v2" }]],
    ["commits", [{ sha: "a".repeat(40), commit: { message: "Move the city\n\nDetails" } }]],
  ]);
  const items = await loadRevisions("owner/project", async url =>
    Response.json(fixture.get(new URL(url).pathname.split("/").at(-1))));
  assert.deepEqual(items, [
    { ref: "refs/heads/feature/flight", label: "Branch · feature/flight" },
    { ref: "refs/tags/v2", label: "Tag · v2" },
    { ref: "a".repeat(40), label: "aaaaaaa · Move the city" },
  ]);
});

test("commit search uses literal keywords and returns matches from the requested repository", async () => {
  const sha = "a".repeat(40);
  const items = await searchCommits("Owner/Project", 'Tune the motor: "noise"', async url => {
    const target = new URL(url);
    assert.equal(target.pathname, "/search/commits");
    assert.equal(target.searchParams.get("q"), 'repo:Owner/Project "Tune" "the" "motor" "noise"');
    assert.equal(target.searchParams.get("per_page"), "8");
    return Response.json({ items: [
      { sha, commit: { message: "Tune the motor\nDetails" }, repository: { full_name: "owner/project" } },
      { sha: "b".repeat(40), commit: { message: "Tune the motor" }, repository: { full_name: "other/project" } },
    ] });
  });
  assert.deepEqual(items, [{ ref: sha, label: "aaaaaaa · Tune the motor" }]);
});
