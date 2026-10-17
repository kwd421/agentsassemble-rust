import { afterEach, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs(); localStorage.clear(); sessionStorage.clear(); });

const anchorFor = (server_id: string) => ({
  server_id, registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
  endpoint_origin: "https://host.test", endpoint_generation: 3,
  rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
});

it("clears only the signed-out account's member anchors", async () => {
  vi.resetModules();
  const { clearCentralSession, saveSession } = await import("./identity");
  const { saveMemberAnchor, loadMemberAnchors } = await import("./memberAnchors");
  saveMemberAnchor("person-a", anchorFor("server-a"));
  saveMemberAnchor("person-b", anchorFor("server-b"));
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
    token: "fixture-session", expires_at: 9_999_999_999, device_id: "device",
    person: { person_id: "person-a", identity_kind: "google", display_name: "A" },
  }));
  clearCentralSession();
  expect(loadMemberAnchors("person-a")).toEqual([]);
  expect(loadMemberAnchors("person-b")).toHaveLength(1);
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
    token: "other-session", expires_at: 9_999_999_999, device_id: "device",
    person: { person_id: "person-b", identity_kind: "google", display_name: "B" },
  }));
  saveSession({
    session: { token: "new-session", expires_at: 9_999_999_999, device_id: "device" },
    person: { person_id: "person-c", identity_kind: "google", display_name: "C" },
  });
  expect(loadMemberAnchors("person-b")).toEqual([]);
});

it("clears the local session even when the server revoke cannot be sent, without retrying it", async () => {
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
  const { saveMemberAnchor, loadMemberAnchors } = await import("./memberAnchors");
  saveMemberAnchor("fixture-person", anchorFor("server-a"));
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ token: "fixture-session", expires_at: 9_999_999_999, device_id: device.deviceId, person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" } }));
  const { logoutCentral, loadCentralSession, centralSessionLoggedOut } = await import("./identity");
  // An unsent revoke never keeps the user logged in: logout resolves and
  // the local session (plus that account's anchors) is gone. The unrevoked
  // server-side session simply expires at expires_at; nothing is retried.
  await logoutCentral();
  expect(fetcher).toHaveBeenCalledOnce();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
  expect(loadMemberAnchors("fixture-person")).toEqual([]);
  await logoutCentral();
  expect(fetcher).toHaveBeenCalledOnce();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
});

it("quota-blocked session revokes nothing over the network yet logs out locally", async () => {
  vi.resetModules();
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
  const fetcher = vi.fn(async () => Response.json({ ok: true }));
  vi.stubGlobal("fetch", fetcher);
  const { noteCentralDailyQuotaExhausted, resetCentralRequestGovernorForTests } =
    await import("./centralRequestGovernor");
  resetCentralRequestGovernorForTests();
  const { saveMemberAnchor, loadMemberAnchors } = await import("./memberAnchors");
  saveMemberAnchor("fixture-person", anchorFor("server-a"));
  saveMemberAnchor("other-person", anchorFor("server-b"));
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ token: "fixture-session", expires_at: 9_999_999_999, device_id: "fixture-device", person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" } }));
  // The governor parks every central request until 00:00 UTC once the
  // daily allowance is spent; the logout revoke stays unsent.
  noteCentralDailyQuotaExhausted("fixture-person");
  const { logoutCentral, loadCentralSession, centralSessionLoggedOut } = await import("./identity");
  await logoutCentral();
  expect(fetcher).not.toHaveBeenCalled();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
  expect(loadMemberAnchors("fixture-person")).toEqual([]);
  expect(loadMemberAnchors("other-person")).toHaveLength(1);
  resetCentralRequestGovernorForTests();
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

it.each([
  ["/v1/account/deletion-proof", "fresh_google_authentication_required"],
  ["/v1/account/deletion-proof", "account_deletion_reauth_required"],
  ["/v1/account/deletion-proof", "invalid_session"],
  ["/v1/account", "account_deletion_reauth_required"],
  ["/v1/account", "invalid_session"],
  ["/v1/account", "authentication_required"],
  ["/v1/account", "invalid_signed_request"],
  ["/v1/logout", "account_deletion_reauth_required"],
  ["/v1/account", "rate_limited", 429],
  ["/v1/account", "abuse_limiter_unavailable", 503],
])("keeps exact step-up failures retryable but clears invalid authentication: %s %s", async (path, code, responseStatus = 401) => {
  vi.resetModules();
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  const device = { deviceId: "fixture-device", privateKey: pair.privateKey, publicJwk: await crypto.subtle.exportKey("jwk", pair.publicKey) };
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: { close() {}, transaction: () => ({ objectStore: () => ({ get: () => {
      const get = { result: device, onsuccess: () => {} }; queueMicrotask(() => get.onsuccess()); return get;
    } }) }) }, onsuccess: () => {} };
    queueMicrotask(() => request.onsuccess()); return request;
  } });
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ error: { code, message: "확인하지 못했어요." } }, { status: responseStatus })));
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ token: "fixture-session", expires_at: 9_999_999_999, device_id: device.deviceId, person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" } }));
  const { signedRequest, loadCentralSession } = await import("./identity");
  const session = loadCentralSession()!;
  await expect(signedRequest(session, path, path === "/v1/account" ? "DELETE" : "POST", {})).rejects.toMatchObject({ status: responseStatus, code });
  expect(loadCentralSession()?.token ?? null).toBe(["invalid_session", "authentication_required", "invalid_signed_request"].includes(code) || path === "/v1/logout" ? null : "fixture-session");
});
