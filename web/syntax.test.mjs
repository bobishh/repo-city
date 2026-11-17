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

test("new source formats load their grammars on demand and preserve Unicode byte offsets", async () => {
  for (const [language, source, expected] of [
    ["python", 'def read():\n    return "привет 🦀"', "return"],
    ["bash", '#!/bin/bash\nif true; then echo "город"; fi', "if"],
    ["css", '.city { color: "cyan"; /* город */ }', "город"],
    ["html", '<section title="город">hello</section>', "section"],
    ["yaml", 'city: "город"\nfiles: 42', "город"],
    ["markdown", '# City\n\nRead `code` in the city.\n<!-- город -->', "City"],
  ]) {
    const spans = await tokenize(language, source);
    assert.ok(spans.length > 0, `${language} should load a real grammar`);
    const bytes = Buffer.from(source);
    const text = [];
    for (let i = 0; i < spans.length; i += 3) {
      assert.ok(spans[i] < spans[i + 1] && spans[i + 1] <= bytes.length, `${language}: valid byte range`);
      text.push(new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(spans[i], spans[i + 1])));
    }
    assert.ok(text.some(token => token.includes(expected)), `${language}: expected token ${expected}`);
  }
  assert.deepEqual(await tokenize("unsupported-format", "readable source"), new Uint32Array());
});

test("file extensions request asynchronous syntax work including existing mjs aliases", async () => {
  const { createSyntaxCache } = await import("./syntax.mjs");
  const original = globalThis.Worker;
  const messages = [];
  globalThis.Worker = class {
    addEventListener() {}
    postMessage(message) { messages.push(message); }
  };
  try {
    const cache = createSyntaxCache();
    for (const [path, language] of [
      ["src/main.PY", "python"], ["types.pyi", "python"], ["run.sh", "bash"],
      [".bashrc", "bash"], ["city.css", "css"], ["index.html", "html"],
      ["config.yaml", "yaml"], ["config.yml", "yaml"], ["README.md", "markdown"],
      ["notes.markdown", "markdown"], ["syntax-worker.mjs", "javascript"],
      ["config.cjs", "javascript"],
    ]) {
      assert.equal(cache.spans(path, `source for ${path}`).length, 0, "source stays readable while pending");
      assert.equal(messages.at(-1)?.language, language, path);
    }
  } finally {
    globalThis.Worker = original;
  }
});
