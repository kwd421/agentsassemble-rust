import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";

const expectedCss = new Map([
  [
    "CustomChannelView-BZfYj86-.css",
    "f10dd6c3e344f04c7895e85be39cf1eb173ff9e597b8d9ff38f40bcd8cae3b4c",
  ],
  [
    "index-DkZ0picp.css",
    "ec5e1ef2db1793bcc1a326ae07a1b9ddfc3499f89dd3ab8f0374fe5386cc0331",
  ],
  [
    "useRoomMessageSearch-BBKeNGgP.css",
    "21d5c6fab91b09acda78881428fb38a26483184f260603d23261da5701ad9d28",
  ],
]);

const assetsDirectory = resolve("dist/assets");
const actualNames = (await readdir(assetsDirectory))
  .filter((name) => name.endsWith(".css"))
  .sort();
const expectedNames = [...expectedCss.keys()].sort();

if (JSON.stringify(actualNames) !== JSON.stringify(expectedNames)) {
  throw new Error(
    `approved frontend CSS chunks changed: expected ${expectedNames.join(", ")}; got ${actualNames.join(", ")}`,
  );
}

for (const name of expectedNames) {
  const bytes = await readFile(resolve(assetsDirectory, name));
  const actualHash = createHash("sha256").update(bytes).digest("hex");
  const expectedHash = expectedCss.get(name);
  if (actualHash !== expectedHash) {
    throw new Error(
      `${name} no longer matches the approved frontend CSS cascade: expected ${expectedHash}; got ${actualHash}`,
    );
  }
}

console.log("verified approved frontend CSS chunks and cascade");
