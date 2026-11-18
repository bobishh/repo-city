import assert from "node:assert/strict";
import { test } from "node:test";
import { containsRange, writeReviewComment } from "./pr-review.mjs";

const sha = "a".repeat(40);
const patch = "@@ -2,3 +2,4 @@\n context\n-old\n+new\n+another\n after\n@@ -18 +19 @@\n-other\n+next";
const path = "src/lib.rs";
const comment = { id: "f20ca9be-aeb0-4238-b3e7-a40af23486ba", revision: sha, path, before: false, start: 2, end: 4, preset: "tests", text: "Test both outcomes." };

test("publishes comments on closed and merged pull requests without bypassing diff validation", async () => {
  for (const merged of [false, true]) {
    let posted;
    const call = async (suffix, payload) => {
      if (!suffix) return Response.json({ state: "closed", merged, head: { sha } });
      if (suffix.startsWith("/files")) return Response.json([{ filename: path, patch }]);
      if (!payload) return Response.json([]);
      posted = payload;
      return Response.json({ id: 42 });
    };
    // Given a historical PR at its actual head, valid coordinates can still publish.
    assert.equal((await writeReviewComment(comment, "reviewer", call)).status, 201);
    assert.equal(posted.commit_id, sha);
    assert.equal(posted.line, 4);
    posted = undefined;
    // Stale revisions and invalid lines remain private, irrespective of PR state.
    assert.equal((await writeReviewComment({ ...comment, revision: "b".repeat(40) }, "reviewer", call)).status, 409);
    assert.equal((await writeReviewComment({ ...comment, end: 19 }, "reviewer", call)).status, 422);
    assert.equal(posted, undefined);
  }
});

test("validates the whole inclusive range on one GitHub diff side", () => {
  assert.equal(containsRange(patch, false, 2, 5), true);
  assert.equal(containsRange(patch, true, 2, 4), true);
  assert.equal(containsRange(patch, true, 2, 5), false);
  assert.equal(containsRange(patch, false, 4, 19), false);
  assert.equal(containsRange("something else", false, 2, 2), false);
});

test("publishes a line replacement and keeps invalid suggestion anchors private", async () => {
  const replacement = '    new_code();\n    // ```';
  let posted;
  const call = async (suffix, payload) => {
    if (!suffix) return Response.json({ state: "open", head: { sha } });
    if (suffix.startsWith("/files")) return Response.json([{ filename: path, patch }]);
    if (!payload) return Response.json([]);
    posted = payload;
    return Response.json({ id: 42 });
  };
  const run = fields => writeReviewComment({ ...comment, text: "", replacement, ...fields }, "reviewer", call);
  assert.equal((await run({ before: true })).status, 400);
  assert.equal((await run({ fileLevel: true })).status, 400);
  const outside = await run({ end: 19 });
  assert.equal(outside.status, 422);
  assert.equal(outside.value.fileLevel, undefined);
  assert.equal(posted, undefined);
  assert.equal((await run({})).status, 201);
  assert.equal(posted.side, "RIGHT");
  assert.equal(posted.start_line, 2);
  assert.equal(posted.line, 4);
  assert.match(posted.body, /^````suggestion\n    new_code\(\);\n    \/\/ ```\n````\n\n<!-- repo-city:v1:/);
});

test("publishes explicit diff coordinates, retains stale anchors, and finds a prior uncertain write", async () => {
  let seen, writes = 0, moved = false;
  const call = async (suffix, payload) => {
    if (!suffix) return Response.json({ state: "open", head: { sha: moved ? "b".repeat(40) : sha } });
    if (suffix.startsWith("/files")) return Response.json([{ filename: path, patch }]);
    if (!payload) return Response.json(seen ? [seen] : []);
    writes++;
    assert.deepEqual({ ...payload, body: undefined }, {
      path, commit_id: sha, side: "RIGHT", line: 4, start_side: "RIGHT", start_line: 2, body: undefined,
    });
    assert.match(payload.body, /^Test both outcomes\.\n\n<!-- repo-city:v1:/);
    seen = { id: 42, user: { login: "reviewer" }, body: payload.body, html_url: "https://github.com/owner/repo/pull/5#discussion_r42" };
    throw new Error("GitHub did not confirm the write");
  };
  const run = input => writeReviewComment(input, "reviewer", call);
  assert.equal((await run({ ...comment, end: 18 })).status, 422);
  assert.equal(writes, 0);
  assert.equal((await run(comment)).status, 502);
  moved = true;
  assert.equal((await run(comment)).value.comment.id, 42);
  assert.equal((await run({ ...comment, id: "e20ca9be-aeb0-4238-b3e7-a40af23486ba" })).status, 409);
  assert.equal(writes, 1);
});

test("rejects malformed note identities, GitHub anchors and bodies before any network request", async () => {
  for (const fields of [
    { id: "not-a-note-id" }, { revision: "not-a-commit" }, { preset: "has spaces" },
    { path: "../outside.rs" }, { path: "/absolute.rs" }, { path: "a\u0000.rs" },
    { before: "false" }, { start: 0 }, { start: 1.5 }, { end: 1 },
    { end: Number.MAX_SAFE_INTEGER + 1 }, { fileLevel: "true" },
    { text: "   " }, { text: "x".repeat(6001) }, { text: null },
  ]) {
    assert.deepEqual(await writeReviewComment({ ...comment, ...fields }, "reviewer", () => assert.fail("Invalid input must not reach GitHub")), {
      status: 400, value: { error: "Invalid draft comment." },
    });
  }
});
