import { startServer } from "../server.mjs";

const port = Number(process.argv[2]);
startServer({ port, host: "127.0.0.1", origin: `http://127.0.0.1:${port}` });
