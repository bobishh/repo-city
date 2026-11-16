import test from "node:test";
import assert from "node:assert/strict";
import { carryVisits } from "./progress.mjs";

test("carries file and diff visits across new heads only when their content still matches", () => {
  const visit = (path, diff) => JSON.stringify({ visited: { path, diff } });
  const old = {
    before: { "same.rs": { id: "base-1" }, "new-base.rs": { id: "base-1" } },
    after: Object.fromEntries(["same.rs", "new-base.rs", "rewritten.rs", "gone.rs"].map(path => [path, { id: "head-1" }])),
  };
  const snapshot = {
    before: { "same.rs": { id: "base-1" }, "new-base.rs": { id: "base-2" } },
    after: { "same.rs": { id: "head-1" }, "new-base.rs": { id: "head-1" }, "rewritten.rs": { id: "head-2" } },
  };
  const marks = [visit("same.rs", true), visit("new-base.rs", true), visit("rewritten.rs", false), visit("gone.rs", false),
    '{"reviewed":"same.rs"}'];
  assert.deepEqual(carryVisits(marks, old, snapshot), [
    [visit("same.rs", true), true], [visit("new-base.rs", false), true],
  ]);
});
