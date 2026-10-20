import { expect, it } from "vitest";
// Vitest runs in Node, but the app tsconfig deliberately has no Node types.
// @ts-expect-error built-in module
import { readFileSync } from "node:fs";

const fieldStyles = readFileSync("src/styles/original/segment-01.css", "utf8");

// The harness helper text makes its own field taller. A stretched grid field
// with normal align-content shares that extra height between its label and
// control, so the neighbouring model select dropped out of line.
it("keeps settings fields top-aligned when a helper text grows one of them", () => {
  const fieldRule =
    fieldStyles.match(/(?:^|\n)\.dc-agent-field \{([\s\S]*?)\}/)?.[1] ?? "";
  expect(fieldRule).toContain("display: grid;");
  expect(fieldRule).toContain("align-content: start;");

  const dualRule =
    fieldStyles.match(
      /(?:^|\n)\.dc-agent-field-grid--dual \{([\s\S]*?)\}/
    )?.[1] ?? "";
  expect(dualRule).toContain("grid-template-columns: repeat(2, minmax(0, 1fr));");
});
