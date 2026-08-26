import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getOrCreateBrowserCredential } from "./deviceIdentity";

const BROWSER_CREDENTIAL_STORAGE_KEY = "agentsassemble.browserCredential.v1";
const OLD_DEVICE_TOKEN_STORAGE_KEY = "agentsassemble.deviceToken.v1";

describe("browser admission credential", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("generates one canonical 32-byte credential and reuses its durable value", () => {
    const random = vi
      .spyOn(globalThis.crypto, "getRandomValues")
      .mockImplementation((target) => {
        const bytes = target as Uint8Array;
        bytes.forEach((_, index) => {
          bytes[index] = index;
        });
        return target;
      });

    const first = getOrCreateBrowserCredential();
    const second = getOrCreateBrowserCredential();

    expect(first).toMatch(/^aad1_[A-Za-z0-9_-]{43}$/);
    expect(second).toBe(first);
    expect(window.localStorage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY)).toBe(first);
    expect(random).toHaveBeenCalledOnce();
  });

  it("does not import or modify the old device-token value", () => {
    window.localStorage.setItem(OLD_DEVICE_TOKEN_STORAGE_KEY, "legacy-device-token");
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation((target) => {
      (target as Uint8Array).fill(0xa7);
      return target;
    });

    const credential = getOrCreateBrowserCredential();

    expect(credential).toMatch(/^aad1_[A-Za-z0-9_-]{43}$/);
    expect(credential).not.toContain("legacy-device-token");
    expect(window.localStorage.getItem(OLD_DEVICE_TOKEN_STORAGE_KEY)).toBe(
      "legacy-device-token"
    );
  });

  it("rejects a malformed stored value without replacing it", () => {
    window.localStorage.setItem(BROWSER_CREDENTIAL_STORAGE_KEY, "aad1_not-canonical");
    const random = vi.spyOn(globalThis.crypto, "getRandomValues");

    expect(() => getOrCreateBrowserCredential()).toThrow(/손상/);
    expect(window.localStorage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY)).toBe(
      "aad1_not-canonical"
    );
    expect(random).not.toHaveBeenCalled();
  });

  it("fails closed when WebCrypto is unavailable", () => {
    vi.stubGlobal("crypto", {});

    expect(() => getOrCreateBrowserCredential()).toThrow(/영구 저장/);
    expect(window.localStorage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY)).toBeNull();
  });

  it("fails closed when durable storage cannot confirm the write", () => {
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation((target) => {
      (target as Uint8Array).fill(0x5c);
      return target;
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => undefined);

    expect(() => getOrCreateBrowserCredential()).toThrow(/영구 저장/);
    expect(window.localStorage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY)).toBeNull();
  });
});
