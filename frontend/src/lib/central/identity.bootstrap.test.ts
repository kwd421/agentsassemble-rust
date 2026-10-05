import { afterEach, expect, it, vi } from "vitest";

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); });

it.each(["fetch", "body", "cancel"])("preserves the saved session after %s interruption", async stage => {
  vi.resetModules();
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  const device = { deviceId: "fixture-device", privateKey: pair.privateKey, publicJwk: await crypto.subtle.exportKey("jwk", pair.publicKey) };
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: { close: vi.fn(), transaction: () => ({ objectStore: () => ({ get: () => {
      const read = { result: device, onsuccess: () => {} };
      queueMicrotask(() => read.onsuccess()); return read;
    } }) }) }, onsuccess: () => {} };
    queueMicrotask(() => request.onsuccess()); return request;
  } });
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ token: "fixture-session", expires_at: 9_999_999_999,
    device_id: device.deviceId, person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" } }));
  const timeout = new AbortController();
  const caller = new AbortController();
  vi.spyOn(AbortSignal, "timeout").mockReturnValue(timeout.signal);
  const raw = new DOMException("Fetch is aborted", "AbortError");
  const interrupt = () => {
    if (stage === "cancel") caller.abort();
    else timeout.abort(new DOMException("Timed out", "TimeoutError"));
    throw raw;
  };
  vi.stubGlobal("fetch", vi.fn(async () => {
    if (stage === "fetch") return interrupt();
    return { ok: true, status: 200, json: async () => interrupt() };
  }));
  const { bootstrapCentral, loadCentralSession } = await import("./identity");
  const { isCentralTemporaryError } = await import("./connectionError");
  const error = await bootstrapCentral(caller.signal).catch(e => e);
  expect(isCentralTemporaryError(error)).toBe(stage !== "cancel");
  if (stage === "cancel") expect(error).toBe(raw);
  expect(loadCentralSession()?.token).toBe("fixture-session");
});
