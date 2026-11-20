// Session wire format from web/github-auth.mjs at 306a6c5.
import { createCipheriv, createHash, randomBytes } from "node:crypto";

const key = createHash("sha256").update("repo-city:github:v1:test-secret").digest();
const iv = randomBytes(12);
const mode = process.argv[2];
const name = mode === "flow" ? "__Host-repo-city-oauth" : "__Host-repo-city";
const cipher = createCipheriv("aes-256-gcm", key, iv);
cipher.setAAD(Buffer.from(name));

const payload = {
  ...(mode === "flow"
    ? { verifier: "test-verifier", state: "test-state", returnTo: "/?migrate=1" }
    : { login: "reviewer", token: "ghu_fake" }),
  expires: Date.now() + 60_000,
};
const body = Buffer.concat([
  cipher.update(JSON.stringify(payload)),
  cipher.final(),
  cipher.getAuthTag(),
]);
console.log(iv.toString("base64url") + "." + body.toString("base64url"));
