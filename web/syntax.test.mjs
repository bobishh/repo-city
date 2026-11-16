import assert from "node:assert/strict";
import { test } from "node:test";
import { tokenize } from "./syntax-worker.mjs";

test("Rust colors real tokens across lines and measures ranges in UTF-8", async () => {
  const source = '/* 😀\nλ */\nfn main() { let s = "λ🛸"; let n = 12; }';
  const spans = await tokenize("rust", source);
  const bytes = Buffer.from(source);
  const tokens = [];
  for (let i = 0; i < spans.length; i += 3) {
    tokens.push([bytes.subarray(spans[i], spans[i + 1]).toString(), spans[i + 2]]);
  }
  assert.ok(tokens.some(([text, kind]) => text.includes("λ */") && kind === 3));
  assert.ok(tokens.some(([text, kind]) => text === "fn" && kind === 1));
  assert.ok(tokens.some(([text, kind]) => text.includes("λ🛸") && kind === 2));
  assert.ok(tokens.some(([text, kind]) => text === "12" && kind === 4));
});

test("supported languages have token colors and JSON/TOML may be loaded independently", async () => {
  for (const [language, source] of [
    ["go", "package main\nfunc main() { println(42) }"],
    ["typescript", "const count: number = 3"],
    ["ruby", "puts 'robot'"],
    ["elixir", "defmodule City do\n  def demo, do: 123\nend"],
    ["elm", "port module City exposing (main)\nmain = 42"],
    ["toml", 'name = "city"'],
    ["json", '{"name":"city"}'],
  ]) assert.ok((await tokenize(language, source)).length > 0, language);
});
