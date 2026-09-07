import { defineConfig } from "vitest/config";

const workerExecArgv = process.allowedNodeEnvironmentFlags.has("--no-experimental-webstorage")
  ? ["--no-experimental-webstorage"]
  : [];

export default defineConfig({
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/nativeDialog.ts"],
    execArgv: workerExecArgv,
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
