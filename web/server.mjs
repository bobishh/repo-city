// Same-origin HTTP adapter for Kamal. The GitHub session contract lives in github-auth.mjs.
import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createGunzip } from "node:zlib";
import { handleGitHub } from "./github-auth.mjs";

const dist = fileURLToPath(new URL("./dist/", import.meta.url));
const types = {
  ".html": "text/html; charset=utf-8", ".mjs": "text/javascript", ".js": "text/javascript",
  ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm",
  ".webp": "image/webp", ".png": "image/png", ".svg": "image/svg+xml", ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8", ".md": "text/plain; charset=utf-8",
};

async function fileInfo(path) {
  try {
    const info = await stat(path);
    return info.isFile() ? info : null;
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ENOTDIR") return null;
    throw error;
  }
}

export async function serveRequest(request, env = process.env, root = dist) {
  const { pathname } = new URL(request.url);
  if (pathname.startsWith("/github/")) return handleGitHub(request, env);
  if (pathname.startsWith("/party/")) {
    if (pathname === "/party/config" && request.method === "GET") {
      return Response.json({ enabled: false, layout_version: 1 }, { headers: { "Cache-Control": "no-store" } });
    }
    return Response.json({ error: "Review parties are not enabled on this server." }, { status: 503 });
  }
  if (!["GET", "HEAD"].includes(request.method)) {
    return new Response(null, { status: 405, headers: { Allow: "GET, HEAD" } });
  }
  if (pathname === "/up") return new Response(null, { status: 200, headers: { "Cache-Control": "no-store" } });

  let name;
  try {
    name = decodeURIComponent(pathname);
  } catch {
    return new Response(null, { status: 400 });
  }
  if (name.includes("\0")) return new Response(null, { status: 400 });
  let path = resolve(root, name === "/" ? "index.html" : "." + name);
  if (!path.startsWith(resolve(root) + sep)) return new Response(null, { status: 404 });

  const headers = new Headers({
    "Content-Type": types[extname(path)] ?? "application/octet-stream",
    "Cache-Control": "no-cache",
    "X-Content-Type-Options": "nosniff",
  });
  let decompress = false;
  let info = await fileInfo(path);
  if (!info && pathname === "/game/repo_city_bg.wasm") {
    path += ".gz";
    info = await fileInfo(path);
    headers.set("Vary", "Accept-Encoding");
    const acceptsGzip = (request.headers.get("Accept-Encoding") ?? "").split(",").some(item => {
      const [encoding, quality] = item.trim().toLowerCase().split(/\s*;\s*q=/);
      return encoding === "gzip" && (quality === undefined || Number(quality) > 0);
    });
    if (acceptsGzip) headers.set("Content-Encoding", "gzip");
    else decompress = true;
  }
  if (!info) return new Response(null, { status: 404 });
  const etag = 'W/"' + info.size.toString(16) + "-" + Math.trunc(info.mtimeMs).toString(16) + '"';
  headers.set("ETag", etag);
  if (request.headers.get("If-None-Match")?.split(/,\s*/).includes(etag)) {
    return new Response(null, { status: 304, headers });
  }
  if (!decompress) headers.set("Content-Length", String(info.size));
  if (request.method === "HEAD") return new Response(null, { headers });
  const stream = createReadStream(path);
  // Compose propagates cancellation and file errors through the gunzip stage.
  return new Response(Readable.toWeb(decompress ? stream.compose(createGunzip()) : stream), { headers });
}

export function startServer({ port, host, origin, env = process.env, root = dist }) {
  // Use the configured public URL for OAuth redirects and Origin checks (TLS ends at kamal-proxy).
  const publicOrigin = new URL(origin).origin;
  return createServer(async (incoming, outgoing) => {
    outgoing.setHeader("X-Repo-City-Backend", "vps");
    if (env.REPO_CITY_REVISION) outgoing.setHeader("X-Repo-City-Revision", env.REPO_CITY_REVISION);
    try {
      if (!incoming.url.startsWith("/") || incoming.url.startsWith("//")) {
        outgoing.writeHead(400).end();
        return;
      }
      const request = new Request(publicOrigin + incoming.url, {
        method: incoming.method, headers: incoming.headers,
        ...(!["GET", "HEAD"].includes(incoming.method) && { body: incoming, duplex: "half" }),
      });
      const result = await serveRequest(request, env, root);
      const headers = Object.fromEntries(result.headers);
      const cookies = result.headers.getSetCookie();
      if (cookies.length) headers["set-cookie"] = cookies;
      outgoing.writeHead(result.status, headers);
      if (result.body) await pipeline(Readable.fromWeb(result.body), outgoing);
      else outgoing.end();
    } catch {
      if (!outgoing.headersSent && !outgoing.destroyed) outgoing.writeHead(500).end();
      else outgoing.destroy();
    }
  }).listen(port, host);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const server = startServer({
    port: Number(process.env.PORT ?? 8080), host: "0.0.0.0",
    origin: process.env.PUBLIC_ORIGIN ?? "https://repo-city.meta-uber-engineer.dev",
  });
  process.once("SIGTERM", () => server.close());
}
