import { describe, expect, it } from "vitest";

import { isBundledDesktopOrigin } from "./desktopBridge";

describe("bundled desktop origin", () => {
  it("accepts only the exact origins guarded by native Tauri commands", () => {
    expect(isBundledDesktopOrigin("tauri://localhost")).toBe(true);
    expect(isBundledDesktopOrigin("http://tauri.localhost")).toBe(true);
    expect(isBundledDesktopOrigin("https://tauri.localhost")).toBe(true);
    expect(isBundledDesktopOrigin("https://room.example.test")).toBe(false);
    expect(isBundledDesktopOrigin("https://tauri.localhost.evil.test")).toBe(false);
  });
});
