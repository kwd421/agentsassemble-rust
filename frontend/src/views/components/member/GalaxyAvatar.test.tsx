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

  it("keeps the whole effect inside a circular, radially faded halo", () => {
    expect(galaxyStyles).toContain(
      "-webkit-mask-image: radial-gradient(circle, black 55%, transparent 72%);"
    );
    expect(galaxyStyles).toContain(
      "mask-image: radial-gradient(circle, black 55%, transparent 72%);"
    );
    expect(galaxyStyles).toMatch(
      /\.galaxy-avatar-field \{[\s\S]*?border-radius: 50%;/
    );
    expect(galaxyStyles).toMatch(
      /\.galaxy-avatar-field \{[\s\S]*?overflow: hidden;/
    );
  });

  it("blends an Ultra row into the halo without a border or hard edge", () => {
    const rowRule =
      galaxyStyles.match(/\.dc-member\[data-galaxy="true"\] \{([\s\S]*?)\}/)?.[1] ??
      "";
    expect(rowRule).not.toMatch(/\b(background|box-shadow|border|animation)\b/);
    expect(galaxyStyles).toMatch(
      /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\] \{[^}]*overflow: visible;/
    );
    const ultraRule =
      galaxyStyles.match(
        /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\] \{([\s\S]*?)\}/
      )?.[1] ?? "";
    expect(ultraRule).toContain("background: none;");
    expect(ultraRule).toContain("box-shadow: none;");
    expect(ultraRule).toContain("animation: none;");

    const nebulaRule =
      galaxyStyles.match(
        /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\]::before \{([\s\S]*?)\}/
      )?.[1] ?? "";
    // The nebula is centred on the 32px avatar (24px inset + 16px radius) and
    // spreads across the whole row rather than ending at the name.
    expect(nebulaRule).toMatch(/54px 54px at 24px 50%/);
    expect(nebulaRule).toMatch(/118% 150% at 0% 50%/);
    expect(nebulaRule).toContain("border-radius: 8px;");
    expect(nebulaRule).toMatch(/mask-image: linear-gradient\(/);
    // The right end keeps a faint violet tint; the fade never reaches clear.
    expect(nebulaRule).toContain("rgba(0, 0, 0, 0.5) 100%");
    expect(nebulaRule).not.toContain("transparent 84%");
    expect(nebulaRule).not.toMatch(/\bborder(?!-radius)\b/);
    expect(nebulaRule).not.toContain("box-shadow");
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

  it("keeps the star field sparse: three repeating patterns, never speckle", () => {
    const starLayers = (selector: string) => {
      const rule =
        galaxyStyles.match(
          new RegExp(`\\.galaxy-avatar-stars--${selector}::before \\{([\\s\\S]*?)\\}`)
        )?.[1] ?? "";
      return (rule.match(/radial-gradient\(circle/g) ?? []).length;
    };
    expect(starLayers("a")).toBe(2);
    expect(starLayers("b")).toBe(1);

    const rowStars =
      galaxyStyles.match(
        /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\]::before \{([\s\S]*?)\}/
      )?.[1] ?? "";
    // Three faint row stars, not a speckled band.
    expect((rowStars.match(/transparent 1\.[456]\d?px\)/g) ?? []).length).toBe(3);
  });

  it("turns the motion off for reduced-motion readers", () => {
    expect(galaxyStyles).toContain("@media (prefers-reduced-motion: reduce)");
    expect(galaxyStyles).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?animation: none;/
    );
    expect(galaxyStyles).toMatch(
      /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\.galaxy-avatar-meteor \{\s*display: none;/
    );
  });
});
