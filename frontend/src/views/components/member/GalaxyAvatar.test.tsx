import { cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
// Vitest runs in Node, but the app tsconfig deliberately has no Node types.
// @ts-expect-error built-in module
import { readFileSync } from "node:fs";
import { isGalaxyAgent } from "./memberHelpers";

const memberStyles = readFileSync(
  "src/styles/original/segment-06.css",
  "utf8"
);

afterEach(cleanup);

describe("GalaxyAvatar", () => {
  it("only treats maximum reasoning as a galaxy agent", () => {
    expect(isGalaxyAgent({ reasoning_effort: "ultra" })).toBe(true);
    expect(isGalaxyAgent({ reasoning_effort: "ultracode" })).toBe(true);
    expect(isGalaxyAgent({ reasoning_effort: "high" })).toBe(false);
    expect(isGalaxyAgent({ reasoning_effort: "" })).toBe(false);
    expect(isGalaxyAgent(undefined)).toBe(false);
  });

  it("uses 15px member names without changing the row height rules", () => {
    const nameRule =
      memberStyles.match(/(?:^|\n)\.dc-member-name \{([\s\S]*?)\}/)?.[1] ?? "";
    expect(nameRule).toContain("font-size: 15px;");
    expect(nameRule).toContain("font-weight: 500;");
    expect(nameRule).toContain("line-height: 20px;");
    expect(memberStyles).toMatch(
      /(?:^|\n)\.dc-member \{[\s\S]*?min-height: 42px;/
    );
  });
});
