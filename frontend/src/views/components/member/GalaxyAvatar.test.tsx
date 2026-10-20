import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
// Vitest runs in Node, but the app tsconfig deliberately has no Node types.
// @ts-expect-error built-in module
import { readFileSync } from "node:fs";
import GalaxyAvatar from "./GalaxyAvatar";
import { isGalaxyAgent } from "./memberHelpers";

const galaxyStyles = readFileSync(
  "src/views/components/member/GalaxyAvatar.css",
  "utf8"
);
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

  it("keeps small avatars to a halo and gives profile avatars the full effect", () => {
    const view = render(
      <GalaxyAvatar size={32}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    const compact = view.container.querySelector(".galaxy-avatar")!;
    expect(compact.getAttribute("data-galaxy-size")).toBe("compact");
    expect(compact.querySelector(".galaxy-avatar-meteor")).toBeNull();
    expect(compact.textContent).toBe("avatar");

    view.rerender(
      <GalaxyAvatar size={80}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    const full = view.container.querySelector(".galaxy-avatar")!;
    expect(full.getAttribute("data-galaxy-size")).toBe("full");
    expect(full.querySelector(".galaxy-avatar-meteor")).toBeTruthy();
  });

  it("keeps the orbit inside the row and the row card free of borders", () => {
    // The member row pads the 32px avatar by 8px, so the compact orbit may reach
    // at most that far; nothing spills over the owner spine or out of the row.
    const reach = Number(galaxyStyles.match(/--galaxy-reach: (\d+)px;/)?.[1]);
    expect(reach).toBeGreaterThan(0);
    expect(reach).toBeLessThanOrEqual(8);
    expect(galaxyStyles).toMatch(/\.galaxy-avatar-field \{[\s\S]*?inset: calc\(var\(--galaxy-reach\) \* -1\);/);
    const card =
      galaxyStyles.match(
        /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\]::before \{([\s\S]*?)\}/
      )?.[1] ?? "";
    expect(card).toContain("inset: 0;");
    expect(card).toContain("border-radius: 8px;");
    expect(card).toMatch(/radial-gradient\(circle at 24px 50%/);
    expect(card).not.toMatch(/\bborder(?!-radius)\b/);
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

  it("draws two spiral arms, a rim and three evenly spread bright stars", () => {
    const view = render(
      <GalaxyAvatar size={32}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    const container = view.container;
    expect(container.querySelectorAll(".galaxy-avatar-arm")).toHaveLength(2);
    expect(container.querySelector(".galaxy-avatar-ring")).toBeTruthy();
    expect(container.querySelectorAll(".galaxy-avatar-bright")).toHaveLength(3);

    view.rerender(
      <GalaxyAvatar size={80}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    expect(container.querySelectorAll(".galaxy-avatar-bright")).toHaveLength(3);
    expect(container.querySelector(".galaxy-avatar-meteor")).toBeTruthy();
  });

  it("turns the motion off for reduced-motion readers", () => {
    expect(galaxyStyles).toContain("@media (prefers-reduced-motion: reduce)");
    expect(galaxyStyles).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?animation: none !important;/
    );
    expect(galaxyStyles).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\.galaxy-avatar-meteor \{\s*display: none;/
    );
  });
});
