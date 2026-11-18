import assert from "node:assert/strict";
import { test } from "node:test";
import { loadCommitHistory, loadCommitDetails, loadGitHub } from "./github.mjs";
const older = "a".repeat(40), newer = "b".repeat(40);
test("history pages and commit details stay within the selected repository", async () => {
  const calls = [];
  const request = async url => {
    calls.push(new URL(url));
    if (new URL(url).pathname.endsWith(`/commits/${older}`)) return Response.json({ sha: older, commit: { message: "Older", author: { name: "Writer", date: "2026-11-18T12:00:00Z" } }, files: [{ filename: "main.rs", patch: "@@ -1 +1 @@\n-before\n+after" }] });
    return Response.json([{ sha: older, commit: { message: "Older\nDetails", author: { name: "Writer", date: "2026-11-18T12:00:00Z" } } }]);
  };
  const rows = await loadCommitHistory("owner/project", newer, 2, request);
  assert.equal(rows[0].summary, "Older");
  assert.equal(calls[0].searchParams.get("page"), "2");
  assert.equal(calls[0].searchParams.get("sha"), newer);
  assert.match(await loadCommitDetails("owner/project", older, request), /main.rs[\s\S]*-before\n\+after/);
  await assert.rejects(loadCommitDetails("owner/project", "../other", request));
  assert.equal(calls.length, 2);
});
test("explicit Before and After selection compares the chosen trees, not their merge base", async () => {
  const calls = [];
  const repo = await loadGitHub("owner/project", newer, older, async url => {
    const path = new URL(url).pathname; calls.push(path);
    if (path.includes("/commits/")) return Response.json({ sha: path.endsWith(older) ? older : newer, parents: [] });
    if (path.includes("/git/trees/")) return Response.json({ tree: [{ path: "main.rs", type: "blob", sha: path.includes(older) ? older : newer, size: 1, mode: "100644" }] });
    throw new Error(`Unexpected ${path}`);
  }, { exactBase: true });
  assert.equal(JSON.parse(repo.snapshot).baseline, older);
  assert.ok(calls.every(path => !path.includes("/compare/")));
});
