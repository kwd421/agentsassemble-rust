import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";

const expectedCss = new Map([
  [
    "index-2Xj9di4h.css",
    "7be05ed3a3e55ca35b206be053e5ece215e5ae96ccdc4e7ea64c228219d16d5d",
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
