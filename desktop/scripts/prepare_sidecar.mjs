import { copyFileSync, mkdirSync, chmodSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(desktopRoot, "..");
const release = process.argv.includes("--release");
const rustc = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
if (rustc.status !== 0) throw new Error(rustc.stderr || "rustc -vV failed");
const host = rustc.stdout.match(/^host: (.+)$/m)?.[1];
if (!host) throw new Error("rustc did not report a host target");

const buildArgs = ["build", "-p", "agentsassemble-server"];
if (release) buildArgs.push("--release");
const build = spawnSync("cargo", buildArgs, { cwd: repositoryRoot, stdio: "inherit" });
if (build.status !== 0) process.exit(build.status ?? 1);

const executable = process.platform === "win32" ? "agentsassemble-server.exe" : "agentsassemble-server";
const source = join(repositoryRoot, "target", release ? "release" : "debug", executable);
const destinationName = process.platform === "win32"
  ? `agentsassemble-server-${host}.exe`
  : `agentsassemble-server-${host}`;
const destination = join(desktopRoot, "src-tauri", "binaries", destinationName);
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(source, destination);
if (process.platform !== "win32") chmodSync(destination, 0o755);
