import { copyFileSync, mkdirSync, chmodSync, readFileSync } from "node:fs";
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

// Build the standalone supervisor before Tauri's final app build consumes it.
// Only this helper compilation omits its own not-yet-built external binary;
// the normal app configuration still requires and packages both binaries.
const supervisorName = "agentsassemble-runtime-supervisor";
const config = JSON.parse(readFileSync(join(desktopRoot, "src-tauri", "tauri.conf.json"), "utf8"));
const inheritedConfig = JSON.parse(process.env.TAURI_CONFIG || "{}");
const helperConfig = {
  ...inheritedConfig,
  bundle: {
    ...inheritedConfig.bundle,
    externalBin: (inheritedConfig.bundle?.externalBin || config.bundle.externalBin)
      .filter((entry) => entry !== `binaries/${supervisorName}`),
  },
};
const helperArgs = ["build", "--manifest-path", join(desktopRoot, "src-tauri", "Cargo.toml"),
  "--bin", supervisorName, "--target-dir", join(repositoryRoot, "target")];
if (release) helperArgs.push("--release");
const helperBuild = spawnSync("cargo", helperArgs, {
  cwd: repositoryRoot,
  env: { ...process.env, TAURI_CONFIG: JSON.stringify(helperConfig) },
  stdio: "inherit",
});
if (helperBuild.status !== 0) process.exit(helperBuild.status ?? 1);
const suffix = process.platform === "win32" ? ".exe" : "";
const helperSource = join(repositoryRoot, "target", release ? "release" : "debug", `${supervisorName}${suffix}`);
const helperDestination = join(desktopRoot, "src-tauri", "binaries", `${supervisorName}-${host}${suffix}`);
copyFileSync(helperSource, helperDestination);
if (process.platform !== "win32") chmodSync(helperDestination, 0o755);
