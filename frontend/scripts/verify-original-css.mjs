import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";

const expectedCss = new Map([
  [
    "index-pH8hucDd.css",
    "e5ef7b9513be52e1738bcbcc3ce67eebb5354b3a13ade2b055e0aab3ad8fcd88",
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
