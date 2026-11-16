const languages = new Map(Object.entries({
  rs: "rust", js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "jsx",
  ts: "typescript", tsx: "tsx", rb: "ruby", rake: "ruby", ex: "elixir", exs: "elixir",
  elm: "elm", toml: "toml", json: "json",
}));
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
      const language = languages.get(path.split(".").at(-1)?.toLowerCase());
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
