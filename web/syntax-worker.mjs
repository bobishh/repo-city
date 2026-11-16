import { createHighlighterCore } from "@shikijs/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";

const grammars = {
  rust: () => import("@shikijs/langs/rust"),
  go: () => import("@shikijs/langs/go"),
  javascript: () => import("@shikijs/langs/javascript"),
  typescript: () => import("@shikijs/langs/typescript"),
  tsx: () => import("@shikijs/langs/tsx"),
  jsx: () => import("@shikijs/langs/jsx"),
  ruby: () => import("@shikijs/langs/ruby"),
  elixir: () => import("@shikijs/langs/elixir"),
  elm: () => import("@shikijs/langs/elm"),
  toml: () => import("@shikijs/langs/toml"),
  json: () => import("@shikijs/langs/json"),
};

// Tags for Rust's shared palette. Diff backgrounds are selected independently.
const scopes = [
  "keyword, storage, constant.language",
  "string",
  "comment",
  "constant.numeric",
  "entity.name.type, support.type, support.class",
  "entity.name.function, support.function",
];
const tag = index => `#${(index + 1).toString().padStart(6, "0")}`;
const kinds = new Map(scopes.map((_, index) => [tag(index), index + 1]));
const theme = {
  name: "repo-city",
  type: "dark",
  colors: { "editor.foreground": "#ffffff", "editor.background": "#000000" },
  tokenColors: scopes.map((scope, index) => ({ scope, settings: { foreground: tag(index) } })),
};

let core;
const loaded = new Map();
const encoder = new TextEncoder();

export async function tokenize(language, source) {
  if (!Object.hasOwn(grammars, language)) return new Uint32Array();
  core ??= createHighlighterCore({ themes: [theme], langs: [], engine: createJavaScriptRegexEngine() });
  const highlighter = await core;
  if (!loaded.has(language)) loaded.set(language, highlighter.loadLanguage(grammars[language]()));
  await loaded.get(language);
  const input = source.replace(/\r\n?/g, match => match.length === 2 ? "\n" : " ");
  const tokens = highlighter.codeToTokensBase(input, { lang: language, theme: theme.name });
  const rawLines = source.split("\n");
  const spans = [];
  let byte = 0;
  for (const [line, row] of tokens.entries()) {
    for (const token of row) {
      const end = byte + encoder.encode(token.content).length;
      const kind = kinds.get(token.color?.toLowerCase());
      if (kind && byte < end) spans.push(byte, end, kind);
      byte = end;
    }
    byte += rawLines[line]?.endsWith("\r") ? 2 : 1;
  }
  return Uint32Array.from(spans);
}

if (typeof self !== "undefined" && typeof document === "undefined") {
  self.onmessage = async ({ data: { id, language, source } }) => {
    // Failure leaves the readable, uncolored source on the facade.
    const spans = await tokenize(language, source).catch(() => new Uint32Array());
    self.postMessage({ id, spans }, [spans.buffer]);
  };
}
