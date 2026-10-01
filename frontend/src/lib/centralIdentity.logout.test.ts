import { afterEach, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); localStorage.clear(); });

it("keeps a failed logout retryable and persists logout only after a signed revocation", async () => {
  vi.resetModules();
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  const device = { deviceId: "fixture-device", privateKey: pair.privateKey, publicJwk: await crypto.subtle.exportKey("jwk", pair.publicKey) };
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: { close: vi.fn(), transaction: () => ({ objectStore: () => ({ get: () => {
      const read = { result: device, onsuccess: () => {} };
      queueMicrotask(() => read.onsuccess()); return read;
    } }) }) }, onsuccess: () => {} };
    queueMicrotask(() => request.onsuccess()); return request;
  } });
  const fetcher = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(Response.json({ ok: true }));
  vi.stubGlobal("fetch", fetcher);
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ token: "fixture-session", expires_at: 9_999_999_999, device_id: device.deviceId, person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" } }));
  const { logoutCentral, loadCentralSession, centralSessionLoggedOut } = await import("./centralIdentity");
  await expect(logoutCentral()).rejects.toThrow("offline");
  expect(loadCentralSession()?.token).toBe("fixture-session");
  expect(centralSessionLoggedOut()).toBe(false);
  await logoutCentral();
  expect(fetcher.mock.calls[1][0]).toMatch(/\/v1\/logout$/);
  expect(fetcher.mock.calls[1][1].headers["x-aa-signature"]).toBeTruthy();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
});
