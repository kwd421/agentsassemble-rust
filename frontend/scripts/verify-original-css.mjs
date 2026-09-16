import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";

const expectedCss = new Map([
  [
    "index-Bo9GM-mo.css",
    "c38817be9f8783ae6b6e92a437ffa9119355f4c4f90a592d3339e301bc6bba47",
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
