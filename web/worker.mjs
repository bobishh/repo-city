export default {
  async fetch(request, env) {
    const path = new URL(request.url);
    if (path.pathname === "/game/repo_city_bg.wasm") {
      path.pathname += ".gz";
      const compressed = await env.ASSETS.fetch(new Request(path, request));
      if (compressed.ok || compressed.status === 304) {
        const headers = new Headers(compressed.headers);
        headers.set("Content-Type", "application/wasm");
        headers.set("Content-Encoding", "gzip");
        return new Response(compressed.body, { status: compressed.status, headers, encodeBody: "manual" });
      }
    }
    return env.ASSETS.fetch(request);
  },
};
