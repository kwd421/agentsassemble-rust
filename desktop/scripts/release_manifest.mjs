import { readFileSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Run after downloading the Windows workflow artifact and building/signing macOS.
// Every advertised platform must have both its artifact and signature locally.
const [output, ...entries] = process.argv.slice(2);
if (!output || entries.length !== 2) {
  throw new Error("Usage: node release_manifest.mjs OUTPUT darwin-aarch64=APP.tar.gz windows-x86_64=SETUP.exe");
}
const root = fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url));
const { version } = JSON.parse(readFileSync(root, "utf8"));
const platforms = {};
for (const entry of entries) {
  const separator = entry.indexOf("=");
  const platform = entry.slice(0, separator);
  const artifact = resolve(entry.slice(separator + 1));
  if (!["darwin-aarch64", "windows-x86_64"].includes(platform) || platforms[platform]) {
    throw new Error("Release requires exactly one artifact for each supported platform");
  }
  if (readFileSync(artifact).length === 0) throw new Error("Empty release artifact");
  const signature = readFileSync(`${artifact}.sig`, "utf8").trim();
  const decoded = Buffer.from(signature, "base64").toString("utf8");
  if (!decoded.split("\n").some((line) => line.startsWith("trusted comment:") && line.includes(`version:${version}`))) {
    throw new Error(`Signature must bind ${platform} to version ${version}`);
  }
  platforms[platform] = {
    signature,
    url: `https://github.com/kwd421/agentsassemble-rust/releases/download/desktop-v${version}/${encodeURIComponent(basename(artifact))}`,
  };
}
writeFileSync(output, `${JSON.stringify({ version, notes: "AgentsAssemble 앱 업데이트", platforms }, null, 2)}\n`);
