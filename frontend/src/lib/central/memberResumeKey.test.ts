import { beforeEach, describe, expect, it, vi } from "vitest";
import { installFakeIndexedDb } from "../../test/fakeIndexedDb";
import {
  clearMemberResumeKeys,
  forgetMemberResumeKeyHandles,
  loadMemberResumeKey,
  loadOrCreateMemberResumeKey,
} from "./memberResumeKey";

describe("member resume key storage", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    forgetMemberResumeKeyHandles();
    installFakeIndexedDb();
  });

  it("keeps the private key unextractable and out of the plain browser storage", async () => {
    const key = await loadOrCreateMemberResumeKey("person", "server", "epoch");
    expect(key.privateKey.extractable).toBe(false);
    await expect(crypto.subtle.exportKey("pkcs8", key.privateKey)).rejects.toThrow();
    await expect(crypto.subtle.exportKey("jwk", key.privateKey)).rejects.toThrow();
    for (const storage of [window.localStorage, window.sessionStorage]) {
      for (let index = 0; index < storage.length; index += 1) {
        const value = storage.getItem(storage.key(index) || "") || "";
        expect(value).not.toMatch(/private|BEGIN|resume/i);
      }
    }
    // The same device keeps the same key.
    const again = await loadOrCreateMemberResumeKey("person", "server", "epoch");
    expect(again.publicKey).toBe(key.publicKey);
    expect(again.privateKey).toBe(key.privateKey);
  });

  it("gives every person, server and epoch its own key", async () => {
    const first = await loadOrCreateMemberResumeKey("person-a", "server", "epoch");
    const other = await loadOrCreateMemberResumeKey("person-b", "server", "epoch");
    const moved = await loadOrCreateMemberResumeKey("person-a", "server", "next-epoch");
    const otherHost = await loadOrCreateMemberResumeKey("person-a", "other", "epoch");
    expect(new Set([first.publicKey, other.publicKey, moved.publicKey, otherHost.publicKey]).size).toBe(4);
    expect(await loadMemberResumeKey("person-b", "server", "epoch")).not.toBeNull();
    expect(await loadMemberResumeKey("person-a", "server", "next-epoch")).not.toBeNull();
  });

  it("deletes the keys of one person and leaves the other account alone", async () => {
    await loadOrCreateMemberResumeKey("person-a", "server", "epoch");
    await loadOrCreateMemberResumeKey("person-b", "server", "epoch");
    await clearMemberResumeKeys("person-a");
    expect(await loadMemberResumeKey("person-a", "server", "epoch")).toBeNull();
    expect(await loadMemberResumeKey("person-b", "server", "epoch")).not.toBeNull();
  });

  it("drops the in-memory handle when the session is cleared in another tab", async () => {
    const key = await loadOrCreateMemberResumeKey("person", "server", "epoch");
    expect(await loadMemberResumeKey("person", "server", "epoch")).toBe(key);
    window.dispatchEvent(new Event("storage"));
    // The handle is gone; the stored key is still there for the next session
    // of the same account.
    const reloaded = await loadMemberResumeKey("person", "server", "epoch");
    expect(reloaded).not.toBeNull();
    expect(reloaded?.publicKey).toBe(key.publicKey);
  });

  it("never stores a key for another account on the same browser credential", async () => {
    await loadOrCreateMemberResumeKey("person-a", "server", "epoch");
    // A second device knows only the public key: it can never sign.
    const second = await loadMemberResumeKey("person-b", "server", "epoch");
    expect(second).toBeNull();
    const bytes = new Uint8Array([1, 2, 3]);
    await expect(crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, keyless(), bytes)).rejects.toThrow();
  });
});

function keyless(): CryptoKey {
  // A public key cannot sign: a copied public key opens nothing.
  return { type: "public", algorithm: { name: "ECDSA", namedCurve: "P-256" }, usages: ["verify"], extractable: true } as CryptoKey;
}

describe("the cleared session drops the resume keys", () => {
  beforeEach(() => {
    vi.resetModules();
    window.localStorage.clear();
    window.sessionStorage.clear();
    forgetMemberResumeKeyHandles();
    installFakeIndexedDb();
  });

  it("deletes the keys of the signed-out account", async () => {
    const { loadOrCreateMemberResumeKey: create } = await import("./memberResumeKey");
    await create("person-a", "server", "epoch");
    window.localStorage.setItem(
      "agentsassemble.centralSession.v1",
      JSON.stringify({
        token: "fixture-session",
        expires_at: 9_999_999_999,
        device_id: "fixture-device",
        person: { person_id: "person-a", display_name: "Name", identity_kind: "guest" },
      }),
    );
    const { clearCentralSession } = await import("./identity");
    clearCentralSession();
    await vi.waitFor(async () => {
      const { loadMemberResumeKey: load } = await import("./memberResumeKey");
      expect(await load("person-a", "server", "epoch")).toBeNull();
    });
  });

  it("deletes the keys of the previous account when another person signs in", async () => {
    const { loadOrCreateMemberResumeKey: create } = await import("./memberResumeKey");
    await create("person-a", "server", "epoch");
    window.localStorage.setItem(
      "agentsassemble.centralSession.v1",
      JSON.stringify({
        token: "fixture-session",
        expires_at: 9_999_999_999,
        device_id: "fixture-device",
        person: { person_id: "person-a", display_name: "Name", identity_kind: "guest" },
      }),
    );
    const { saveSession } = await import("./identity");
    saveSession({
      person: { person_id: "person-b", display_name: "Other", identity_kind: "guest" },
      session: { token: "other-session", expires_at: 9_999_999_999, device_id: "fixture-device" },
    });
    await vi.waitFor(async () => {
      const { loadMemberResumeKey: load } = await import("./memberResumeKey");
      expect(await load("person-a", "server", "epoch")).toBeNull();
    });
  });

  it("deletes the keys of an expired session", async () => {
    const { loadOrCreateMemberResumeKey: create } = await import("./memberResumeKey");
    await create("person-a", "server", "epoch");
    window.localStorage.setItem(
      "agentsassemble.centralSession.v1",
      JSON.stringify({
        token: "fixture-session",
        expires_at: 1,
        device_id: "fixture-device",
        person: { person_id: "person-a", display_name: "Name", identity_kind: "guest" },
      }),
    );
    const { loadCentralSession } = await import("./identity");
    expect(loadCentralSession()).toBeNull();
    await vi.waitFor(async () => {
      const { loadMemberResumeKey: load } = await import("./memberResumeKey");
      expect(await load("person-a", "server", "epoch")).toBeNull();
    });
  });
});
