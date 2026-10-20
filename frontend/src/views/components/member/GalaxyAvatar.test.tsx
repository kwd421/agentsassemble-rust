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

  it("leaves the member row chrome alone", () => {
    const rowRule =
      galaxyStyles.match(/\.dc-member\[data-galaxy="true"\] \{([\s\S]*?)\}/)?.[1] ??
      "";
    expect(rowRule).not.toMatch(/\b(background|box-shadow|border|animation)\b/);
    expect(galaxyStyles).toMatch(
      /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\] \{[^}]*overflow: visible;/
    );
    expect(galaxyStyles).toMatch(
      /\.dc-member\[data-galaxy="true"\]\[data-ultra="true"\] \{[^}]*background: none;[^}]*box-shadow: none;[^}]*animation: none;/
    );
  });

  it("draws two spiral arms, a rim and more bright stars at profile size", () => {
    const view = render(
      <GalaxyAvatar size={32}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    const container = view.container;
    expect(container.querySelectorAll(".galaxy-avatar-arm")).toHaveLength(2);
    expect(container.querySelector(".galaxy-avatar-ring")).toBeTruthy();
    expect(container.querySelectorAll(".galaxy-avatar-bright")).toHaveLength(1);

    view.rerender(
      <GalaxyAvatar size={80}>
        <span>avatar</span>
      </GalaxyAvatar>
    );
    expect(container.querySelectorAll(".galaxy-avatar-bright")).toHaveLength(4);
    expect(container.querySelector(".galaxy-avatar-meteor")).toBeTruthy();
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
