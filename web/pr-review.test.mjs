import assert from "node:assert/strict";
import { test } from "node:test";
import { containsRange, writeReviewComment } from "./pr-review.mjs";

const sha = "a".repeat(40);
const patch = "@@ -2,3 +2,4 @@\n context\n-old\n+new\n+another\n after\n@@ -18 +19 @@\n-other\n+next";
const path = "src/lib.rs";
const comment = { id: "f20ca9be-aeb0-4238-b3e7-a40af23486ba", revision: sha, path, before: false, start: 2, end: 4, preset: "tests", text: "Test both outcomes." };

test("validates the whole inclusive range on one GitHub diff side", () => {
  assert.equal(containsRange(patch, false, 2, 5), true);
  assert.equal(containsRange(patch, true, 2, 4), true);
  assert.equal(containsRange(patch, true, 2, 5), false);
  assert.equal(containsRange(patch, false, 4, 19), false);
  assert.equal(containsRange("something else", false, 2, 2), false);
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
