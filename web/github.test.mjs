import assert from "node:assert/strict";
import { test } from "node:test";
import { loadGitHub, parseRepository } from "./github.mjs";

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

test("a truncated tree fails instead of silently dropping files", async () => {
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

test("a closed pull request reports the current support limit", async () => {
  await assert.rejects(loadGitHub("https://github.com/owner/project/pull/17", "", "",
    async () => Response.json({ state: "closed" })), /open pull requests/i);
});
