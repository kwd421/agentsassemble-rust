import { cp, mkdir, readFile, writeFile } from "node:fs/promises";

const manifest = JSON.parse(await readFile(new URL(".openai/hosting.json", import.meta.url), "utf8"));
if (!manifest.project_id || manifest.d1 !== null || manifest.r2 !== null) {
  throw new Error("Reuse the registered stateless Sites project.");
}
await mkdir(new URL("dist/server/", import.meta.url), { recursive: true });
await mkdir(new URL("dist/.openai/", import.meta.url), { recursive: true });
await cp(new URL("src/worker.mjs", import.meta.url), new URL("dist/server/index.js", import.meta.url));
await cp(new URL(".openai/hosting.json", import.meta.url), new URL("dist/.openai/hosting.json", import.meta.url));
await writeFile(new URL("dist/server/wrangler.json", import.meta.url), JSON.stringify({
  name: "agentsassemble-mcp", main: "index.js", compatibility_date: "2026-09-02",
  no_bundle: true, observability: { enabled: false },
}, null, 2) + "\n");
console.log("Built private Sites MCP gateway; no credentials or room state included.");
