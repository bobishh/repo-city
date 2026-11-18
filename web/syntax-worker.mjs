import { createStarryNight } from "@wooorm/starry-night";
import { catalog } from "./.generated/syntax-catalog.mjs";
import { loaders } from "./.generated/syntax-loaders.mjs";

const encoder = new TextEncoder();
const loading = new Map();

async function highlighter(scope) {
  if (!loading.has(scope)) loading.set(scope, (async () => {
    const grammars = [], seen = new Set();
    async function collect(scope) {
      if (seen.has(scope) || !Object.hasOwn(loaders, scope)) return;
      seen.add(scope);
      const { default: grammar } = await loaders[scope]();
      grammars.push(grammar);
      await Promise.all((grammar.dependencies ?? []).map(collect));
    }
    await collect(scope);
    // register() rebuilds Starry Night's registry without retaining its WASM URL.
    // Keep one lazy engine per root grammar so every fetch stays on this origin.
    return createStarryNight(grammars, {
      getOnigurumaUrlFetch: () => new URL("./syntax/onig.wasm", import.meta.url),
    });
  })());
  return loading.get(scope);
}

function color(classes, text, inherited) {
  if (inherited === 2 || inherited === 3) return inherited;
  if (classes.includes("pl-c")) return 3;
  if (classes.some(name => ["pl-s", "pl-pds", "pl-sr"].includes(name))) return 2;
  if (classes.includes("pl-k")) return 1;
  if (classes.includes("pl-en") || classes.includes("pl-e")) return 6;
  if (classes.includes("pl-c1")) {
    if (/^(true|false|nil|null|undefined|self|Self)$/.test(text)) return 1;
    return /^[+-]?(?:[0-9]|\.[0-9])/.test(text) ? 4 : 5;
  }
  if (classes.some(name => ["pl-ent", "pl-smi", "pl-v", "pl-mh"].includes(name))) return 5;
  return inherited;
}

export async function tokenize(language, source) {
  const name = language.toLowerCase();
  const scope = Object.hasOwn(loaders, language) ? language
    : Object.hasOwn(catalog.names, name) ? catalog.names[name] : undefined;
  if (!scope) return new Uint32Array();
  const engine = await highlighter(scope);
  const spans = [];
  let byte = 0;
  function visit(node, inherited = 0, classes = []) {
    if (node.type === "text") {
      const end = byte + encoder.encode(node.value).length;
      const kind = color(classes, node.value, inherited);
      if (kind && byte < end) spans.push(byte, end, kind);
      byte = end;
      return;
    }
    const next = node.properties?.className ?? [];
    const kind = color(next, "", inherited);
    for (const child of node.children ?? []) visit(child, kind, next);
  }
  visit(engine.highlight(source, scope));
  return Uint32Array.from(spans);
}

if (typeof self !== "undefined" && typeof document === "undefined") {
  self.onmessage = async ({ data: { id, language, source } }) => {
    const spans = await tokenize(language, source).catch(() => new Uint32Array());
    self.postMessage({ id, spans }, [spans.buffer]);
  };
}
