import { spawnSync } from "node:child_process";

const identity = process.env.APPLE_SIGNING_IDENTITY?.trim();
if (process.platform !== "darwin" || !identity || identity === "-") {
  throw new Error("Signed macOS packaging requires APPLE_SIGNING_IDENTITY set to a Developer ID identity.");
}
const build = spawnSync("npm", ["run", "build"], {
  cwd: new URL("../", import.meta.url), stdio: "inherit",
});
process.exit(build.status ?? 1);
