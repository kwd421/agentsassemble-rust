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
  const { logoutCentral, loadCentralSession, centralSessionLoggedOut } = await import("./identity");
  await expect(logoutCentral()).rejects.toThrow("offline");
  expect(loadCentralSession()?.token).toBe("fixture-session");
  expect(centralSessionLoggedOut()).toBe(false);
  await logoutCentral();
  expect(fetcher.mock.calls[1][0]).toMatch(/\/v1\/logout$/);
  expect(fetcher.mock.calls[1][1].headers["x-aa-signature"]).toBeTruthy();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
});

it.each(["logged-out", "expired-session"])("replaces a %s account slot only after storage commits and keeps the new slot on retry", async (state) => {
  vi.resetModules();
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  let persisted: unknown = { deviceId: "old-guest-device", privateKey: pair.privateKey, publicJwk: await crypto.subtle.exportKey("jwk", pair.publicKey) };
  let failDeletion = true;
  vi.stubGlobal("indexedDB", { open: () => {
    const open = { result: { close() {}, transaction: () => {
      const tx = { oncomplete() {}, onabort() {}, onerror() {}, error: new Error("storage unavailable"), objectStore: () => ({
        delete: () => { queueMicrotask(() => {
          if (failDeletion) tx.onabort(); else { persisted = undefined; tx.oncomplete(); }
        }); },
        get: () => { const req = { result: persisted, onsuccess() {} }; queueMicrotask(() => req.onsuccess()); return req; },
        put: (value: unknown) => { queueMicrotask(() => { persisted = value; tx.oncomplete(); }); },
      }) };
      return tx;
    } }, onsuccess() {} };
    queueMicrotask(() => open.onsuccess()); return open;
  } });
  localStorage.setItem("agentsassemble.centralSession.v1", state === "logged-out" ? state : JSON.stringify({ token: "expired-fixture-session", expires_at: 1, device_id: "old-guest-device", person: { person_id: "old-guest", display_name: "Guest", identity_kind: "guest" } }));
  const requests: Array<Record<string, unknown>> = [];
  vi.stubGlobal("fetch", vi.fn(async (_: string, init: RequestInit) => {
    requests.push(JSON.parse(String(init.body)));
    throw new Error("offline before login completed");
  }));
  const { createCentralGuest, centralSessionLoggedOut, loadCentralSession } = await import("./identity");
  expect(loadCentralSession()).toBeNull();
  await expect(createCentralGuest("New account")).rejects.toThrow();
  expect(requests).toHaveLength(0);
  expect(centralSessionLoggedOut()).toBe(true);
  failDeletion = false;
  await expect(createCentralGuest("New account")).rejects.toThrow("offline");
  await expect(createCentralGuest("New account")).rejects.toThrow("offline");
  expect(requests[0].device_id).not.toBe("old-guest-device");
  expect(requests[1].device_id).toBe(requests[0].device_id);
  expect(centralSessionLoggedOut()).toBe(true);
});
