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
