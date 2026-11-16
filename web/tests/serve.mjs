import { handleGitHub } from "../github-auth.mjs";
import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";

const root = resolve("dist");
const types = { ".html": "text/html", ".mjs": "text/javascript", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".webp": "image/webp", ".woff2": "font/woff2", ".svg": "image/svg+xml", ".png": "image/png" };
createServer(async (request, response) => {
  const { pathname } = new URL(request.url, "http://localhost");
  if (pathname.startsWith("/github/")) {
    const result = await handleGitHub(new Request(`http://${request.headers.host}${request.url}`, {
      method: request.method, headers: request.headers,
      ...(!["GET", "HEAD"].includes(request.method) && { body: request, duplex: "half" }),
    }), process.env);
    const headers = Object.fromEntries(result.headers);
    const cookies = result.headers.getSetCookie();
    if (cookies.length) headers["set-cookie"] = cookies;
    response.writeHead(result.status, headers);
    response.end(await result.text());
    return;
  }
  let path = resolve(root, pathname === "/" ? "index.html" : `.${pathname}`);
  if (!path.startsWith(root + sep)) {
    response.writeHead(404).end();
    return;
  }
  const type = types[extname(path)] ?? "application/octet-stream";
  const headers = { "Content-Type": type };
  try {
    if (extname(path) === ".wasm" && !(await stat(path).catch(() => null))) {
      path += ".gz";
      headers["Content-Encoding"] = "gzip";
    }
    if (!(await stat(path)).isFile()) throw new Error("Missing file");
    response.writeHead(200, headers);
    createReadStream(path).pipe(response);
  } catch {
    response.writeHead(404).end();
  }
}).listen(Number(process.argv[2]), "127.0.0.1");
