import { beforeEach, describe, expect, it } from "vitest";
import {
  loadPublicIngressPreference,
  savePublicIngressPreference,
} from "./publicIngressPreference";

describe("publicIngressPreference", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("defaults to closed with no recorded choice", () => {
    expect(loadPublicIngressPreference()).toBe(false);
  });

  it("remembers open across loads and closed across loads", () => {
    savePublicIngressPreference(true);
    expect(loadPublicIngressPreference()).toBe(true);
    savePublicIngressPreference(false);
    expect(loadPublicIngressPreference()).toBe(false);
  });

  it("treats a corrupt value as closed", () => {
    window.localStorage.setItem("agentsassemble.publicIngress.open.v1", "https://evil.test/token");
    expect(loadPublicIngressPreference()).toBe(false);
  });

  it("persists only the open/closed choice, never tokens or URLs", () => {
    savePublicIngressPreference(true);
    const raw = window.localStorage.getItem("agentsassemble.publicIngress.open.v1");
    expect(raw).toBe("open");
    savePublicIngressPreference(false);
    expect(window.localStorage.getItem("agentsassemble.publicIngress.open.v1")).toBe("closed");
    expect(window.localStorage.length).toBe(1);
  });
});
