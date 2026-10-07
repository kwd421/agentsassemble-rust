// @vitest-environment-options {"url":"https://central.test/"}
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { saveSession } from "./identity";
import { openCentralOwnedServer } from "./secureOwnerEntry";
import { RemoteTransport } from "../remote/remoteTransport";
import { closeRemoteWorkspace, remoteWorkspaceSnapshot } from "../remote/remoteWorkspace";

const server = { server_id: "server", registration_epoch: "epoch", relation: "owner" as const, alias: "컴퓨터", host_public_key_jwk: { kty: "OKP", crv: "Ed25519", x: "public-key" }, host_key_fingerprint: "fingerprint",
  endpoint: { origin: "https://host.test", generation: 7, mode: "event_secure_v1", protocol: "secure_admission_v1", status: "published" as const } };
const hello = { protocol: "secure_admission_v1", client_public_key: "client-key", channel_id: "channel", origin: "https://host.test", generation: 7, purpose: "owner", registration_epoch: "epoch" };
const listeners = new Set<() => void>();
const transport = { hello, active: true, close: vi.fn(() => { transport.active = false; for (const fn of listeners) fn(); }), onClose: (fn: () => void) => { listeners.add(fn); return () => listeners.delete(fn); }, fetch: vi.fn() };
const central = vi.fn();
beforeEach(async () => {
  localStorage.clear(); sessionStorage.clear(); listeners.clear(); transport.active = true; transport.close.mockClear(); transport.fetch.mockReset(); central.mockReset();
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.test");
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  const device = { deviceId: "device", privateKey: pair.privateKey, publicJwk: {} };
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: { close() {}, transaction: () => ({ objectStore: () => ({ get: () => {
      const read = { result: device, onsuccess() {} }; queueMicrotask(() => read.onsuccess()); return read;
    } }) }) }, onsuccess() {} }; queueMicrotask(() => request.onsuccess()); return request;
  } });
  saveSession({ person: { person_id: "person", display_name: "사용자", identity_kind: "google" }, session: { token: "central-only", device_id: "device", expires_at: 9_999_999_999 } });
  vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as unknown as RemoteTransport);
  vi.stubGlobal("fetch", central);
  transport.fetch.mockResolvedValue(new Response(JSON.stringify({ session_token: `aaos1.${"A".repeat(43)}`, session_id: "10000000-0000-4000-8000-000000000001", server_id: "server", generation: 7 })));
});
afterEach(() => { closeRemoteWorkspace(); vi.restoreAllMocks(); vi.unstubAllEnvs(); vi.unstubAllGlobals(); localStorage.clear(); });
function grant(overrides = {}) { return { ...hello, server_id: "server", grant_token: `aacg1.${"A".repeat(43)}`, expires_at: Math.floor(Date.now() / 1000) + 300, ...overrides }; }
it("pins the central grant to the verified channel and installs the shared workspace without navigation", async () => {
  central.mockResolvedValue(new Response(JSON.stringify(grant()), { status: 201 }));
  await openCentralOwnedServer(server);
  expect(central).toHaveBeenCalledOnce();
  const [url, init] = central.mock.calls[0];
  expect(url).toBe("https://central.test/v1/servers/server/connect-grants");
  expect(JSON.parse(init.body)).toEqual(hello);
  expect(transport.fetch.mock.calls[0][0]).toBe("/api/central-owner/session");
  expect(JSON.stringify(transport.fetch.mock.calls)).not.toContain("central-only");
  expect(remoteWorkspaceSnapshot()?.owner?.serverId).toBe("server");
  expect(window.location.href).toBe("https://central.test/");
});
it("rejects a mismatched channel echo before sending the grant to the host", async () => {
  central.mockResolvedValue(new Response(JSON.stringify(grant({ channel_id: "other" }))));
  await expect(openCentralOwnedServer(server)).rejects.toThrow();
  expect(transport.fetch).not.toHaveBeenCalled(); expect(transport.close).toHaveBeenCalled();
});
