import { catalog } from "./.generated/syntax-catalog.mjs";

export function languageForPath(path, source = "") {
  const name = path.split(/[\\/]/).at(-1).toLowerCase();
  if (Object.hasOwn(catalog.filenames, name)) return catalog.filenames[name];
  for (let dot = name.indexOf("."); dot >= 0; dot = name.indexOf(".", dot + 1)) {
    const extension = name.slice(dot);
    if (Object.hasOwn(catalog.names, extension.slice(1))) return catalog.names[extension.slice(1)];
    if (Object.hasOwn(catalog.extensions, extension)) return catalog.extensions[extension];
  }
  const alias = name.split(".").at(-1);
  if (Object.hasOwn(catalog.names, alias)) return catalog.names[alias];
  if (source.startsWith("#!")) {
    for (const token of source.split(/[\r\n]/, 1)[0].slice(2).trim().split(/\s+/)) {
      const interpreter = token.split("/").at(-1).toLowerCase();
      if (Object.hasOwn(catalog.interpreters, interpreter)) return catalog.interpreters[interpreter];
    }
  }
}

const empty = new Uint32Array();
const CACHE_ENTRIES = 12;

export function createSyntaxCache() {
  const cache = new Map();
  const pending = new Map();
  let worker, failed, version = 0, id = 0;

  function stop() {
    worker?.terminate();
    failed = true;
    pending.clear();
    cache.clear();
  }

  return {
    version: () => version,
    spans(path, source) {
      const language = languageForPath(path, source);
      if (!language || !source || failed) return empty;
      const key = language + "\0" + source;
      const existing = cache.get(key);
      if (existing) {
        cache.delete(key);
        cache.set(key, existing);
        return existing.spans;
      }
      if (pending.size >= CACHE_ENTRIES) return empty;
      try {
        if (!worker) {
          worker = new Worker(new URL("./syntax-worker.mjs", import.meta.url), { type: "module" });
          worker.addEventListener("error", stop);
          worker.onmessage = ({ data: { id, spans } }) => {
            const key = pending.get(id);
            pending.delete(id);
            if (cache.has(key)) cache.get(key).spans = spans;
            version += 1;
          };
        }
        if (cache.size >= CACHE_ENTRIES) cache.delete(cache.keys().next().value);
        cache.set(key, { spans: empty });
        pending.set(++id, key);
        worker.postMessage({ id, language, source });
      } catch {
        stop();
      }
      return empty;
    },
  };
}
