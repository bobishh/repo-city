import { createServer } from "node:http";
import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { extname, resolve, sep } from "node:path";

const root = resolve("dist");
const types = { ".html": "text/html", ".mjs": "text/javascript", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm" };
createServer(async (request, response) => {
  const { pathname } = new URL(request.url, "http://localhost");
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
