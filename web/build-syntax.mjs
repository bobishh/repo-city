import { all, common } from "@wooorm/starry-night";
import { mkdir, readFile, writeFile } from "node:fs/promises";

// Generate lazy imports and filename metadata from the pinned grammar packages.
// Grammar bodies are separate browser chunks, not part of the initial game load.
const generated = new URL("./.generated/", import.meta.url);
const upstream = JSON.parse(await readFile(new URL("./syntax-language-map.json", import.meta.url), "utf8"));
const scopes = new Set(all.map(grammar => grammar.scopeName));
const names = {}, extensions = {}, filenames = {}, interpreters = {};
// Common grammars win ambiguous extensions; the complete catalog stays loadable by name.
for (const grammar of [...all, ...common]) {
  for (const name of grammar.names) names[name.toLowerCase()] = grammar.scopeName;
  for (const extension of [...grammar.extensions, ...(grammar.extensionsWithDot ?? [])]) {
    extensions[extension.toLowerCase()] = grammar.scopeName;
  }
}
for (const language of upstream.languages) {
  if (!scopes.has(language.scope)) continue;
  for (const name of language.filenames) filenames[name.toLowerCase()] = language.scope;
  for (const name of language.interpreters) interpreters[name.toLowerCase()] = language.scope;
}
// Both Lean versions use .lean. Prefer current Lean 4; explicit Lean flags remain available.
if (scopes.has("source.lean4")) {
  extensions[".lean"] = "source.lean4";
  names.lean = "source.lean4";
  names.lean3 = "source.lean";
}
await mkdir(generated, { recursive: true });
await writeFile(new URL("syntax-catalog.mjs", generated),
  `export const catalog = ${JSON.stringify({ names, extensions, filenames, interpreters, count: scopes.size })};\n`);
await writeFile(new URL("syntax-loaders.mjs", generated), "export const loaders = {\n" +
  [...scopes].map(scope => `  ${JSON.stringify(scope)}: () => import(${JSON.stringify("@wooorm/starry-night/" + scope)}),`).join("\n") + "\n};\n");
