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

test("carries identity-bound read fragments for Rust to validate against an updated PR", () => {
  const reveal = JSON.stringify({
    reveal: { path: "changed.rs", before: false, start: 0, end: 1, rows: 1 },
    source: { blob: "head-1", lines: ["fn unchanged() {}"] },
  });
  const legacy = JSON.stringify({
    reveal: { path: "changed.rs", before: false, start: 0, end: 1, rows: 1 },
  });
  const document = JSON.stringify({
    read_document: { path: "guide.md", blob: "doc-1", before: false, rows: 1 },
    source: { blob: "doc-1", lines: ["Read paragraph"] },
  });
  const previous = { before: {}, after: {
    "changed.rs": { id: "head-1" }, "guide.md": { id: "doc-1" },
  } };
  const current = { before: {}, after: {
    "changed.rs": { id: "head-2" }, "guide.md": { id: "doc-2" },
  } };
  assert.deepEqual(carryVisits([reveal, legacy, document], previous, current), [
    [reveal, true], [document, true],
  ]);
  assert.deepEqual(carryVisits([reveal], previous, { before: {}, after: {} }), []);
});
