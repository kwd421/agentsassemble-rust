import { readFileSync, writeFileSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const config = JSON.parse(readFileSync(resolve(root, "desktop/src-tauri/tauri.conf.json")));
const [mode] = process.argv.slice(2);
const gh = (...args) => execFileSync("gh", args, { cwd: root, encoding: "utf8" }).trim();
const repo = process.env.GITHUB_REPOSITORY;
const tag = "desktop-test";
const output = resolve(process.env.RUNNER_TEMP, "desktop-test-config.json");

if (mode === "prepare") {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(config.version);
  if (!match) throw new Error("Test builds require a stable base version");
  const run = process.env.GITHUB_RUN_NUMBER;
  const attempt = process.env.GITHUB_RUN_ATTEMPT;
  if (![run, attempt].every(value => /^[1-9]\d*$/.test(value))) throw new Error("Missing build sequence");
  // Build metadata (+...) is ignored by SemVer precedence; numeric prerelease parts are not.
  const version = `${match[1]}.${match[2]}.${Number(match[3]) + 1}-test.${run}.${attempt}`;
  writeFileSync(output, JSON.stringify({ version }));
  console.log(`Test update version: ${version}`);
} else if (mode === "publish") {
  if (repo !== "kwd421/agentsassemble-rust") throw new Error("Unexpected release repository");
  const { version } = JSON.parse(readFileSync(output));
  const directory = resolve(root, "target/release/bundle/nsis");
  const installers = readdirSync(directory).filter(name => name.endsWith(".exe") && name.includes(`_${version}_`));
  if (installers.length !== 1) throw new Error("Expected one NSIS installer");
  const installer = installers[0];
  const signature = readFileSync(resolve(directory, `${installer}.sig`), "utf8").trim();
  if (!signature) throw new Error("Missing updater signature");
  // A missing tag is the only creation case; API/auth failures must not become creation.
  const releases = JSON.parse(gh("api", "--paginate", "--slurp", `repos/${repo}/releases?per_page=100`)).flat();
  const previous = releases.find(release => release.tag_name === tag);
  if (previous) {
    const prior = previous.assets.find(asset => asset.name === "latest.json");
    if (!prior) throw new Error("Existing test release has no version manifest");
    const manifest = JSON.parse(gh("api", `repos/${repo}/releases/assets/${prior.id}`, "-H", "Accept: application/octet-stream"));
    // PowerShell's SemVer implementation compares numeric prerelease identifiers.
    execFileSync("pwsh", ["-NoProfile", "-Command",
      'if ([System.Management.Automation.SemanticVersion]$env:NEW_TEST_VERSION -le [System.Management.Automation.SemanticVersion]$env:OLD_TEST_VERSION) { throw "Refusing non-increasing test release" }'],
      { env: { ...process.env, NEW_TEST_VERSION: version, OLD_TEST_VERSION: manifest.version }, stdio: "inherit" });
  } else {
    gh("release", "create", tag, "--repo", repo, "--target", execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
      "--prerelease", "--latest=false", "--title", "AgentsAssemble test", "--notes", "Owner test builds");
  }
  const latest = resolve(process.env.RUNNER_TEMP, "latest.json");
  writeFileSync(latest, JSON.stringify({ version, pub_date: new Date().toISOString(), platforms: {
    "windows-x86_64": { signature, url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(installer)}` }
  } }, null, 2));
  // Publish the new, version-named installer first; old manifests remain usable.
  gh("release", "upload", tag, "--repo", repo, resolve(directory, installer), resolve(directory, `${installer}.sig`), "--clobber");
  gh("release", "upload", tag, "--repo", repo, latest, "--clobber");
  const commit = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
  gh("api", "--method", "PATCH", `repos/${repo}/git/refs/tags/${tag}`, "-f", `sha=${commit}`, "-F", "force=true");
  gh("release", "edit", tag, "--repo", repo, "--prerelease", "--latest=false", "--title", `AgentsAssemble ${version}`);
  console.log(`https://github.com/${repo}/releases/tag/${tag}`);
} else {
  throw new Error("Expected prepare or publish");
}
