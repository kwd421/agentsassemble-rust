import { beforeEach, expect, it, vi } from "vitest";
import { encodeBase64Url } from "../base64Url";
import { completeDeletionGoogleReturn, clearDeletionGoogleReturn, deletionGoogleReturnSnapshot } from "./accountDeletionGoogle";
import { loadCentralSession, signedRequest } from "./identity";
vi.mock("./identity", () => ({ CENTRAL_SESSION_CHANGED_EVENT: "changed", CENTRAL_SESSION_CLEARED_EVENT: "cleared", isCentralWebEntry: () => true, loadCentralSession: vi.fn(), signedRequest: vi.fn(), unsignedPost: vi.fn(), parseCentralGoogleHandoff: vi.fn() }));
vi.mock("./accountDeletionHost", () => ({ deletionOwnHost: vi.fn(async () => ({ host: null, hasOwnServers: false })), stopDeletionOwnHost: vi.fn() }));
const session = { token: "fixture-session", device_id: "fixture-device", expires_at: 2 ** 31, person: { person_id: "fixture-person", display_name: "Guest", identity_kind: "google" as const } };
const requestId = "r".repeat(43), state = "s".repeat(43);
beforeEach(async () => {
  deletionGoogleReturnSnapshot()?.operation?.cancel(); clearDeletionGoogleReturn();
  vi.clearAllMocks(); sessionStorage.clear(); vi.mocked(loadCentralSession).mockReturnValue(session);
  const fingerprint = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(session.token)));
  sessionStorage.setItem("agentsassemble.accountDeletionGoogle.v1", JSON.stringify({ requestId, personId: session.person.person_id, deviceId: session.device_id, fingerprint, verifier: "v".repeat(43), state, expiresAt: Math.floor(Date.now() / 1000) + 300, progress: [] }));
  window.history.replaceState({}, "", `/?code=controlled-code&state=${state}`);
});
it("routes fresh-auth return only to signed deletion proof and fresh inventory, sanitizing its URL once", async () => {
  vi.mocked(signedRequest).mockImplementation(async (_s, path) => path.endsWith("deletion-proof") ? { request_id: requestId, proof: "p".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 } : { servers: [] });
  expect(await completeDeletionGoogleReturn()).toBe(true);
  expect(window.location.search).toBe("?account=settings");
  expect(deletionGoogleReturnSnapshot()?.operation?.hasFreshProof).toBe(true);
  expect(deletionGoogleReturnSnapshot()?.operation?.inventoryReady).toBe(true);
  expect(vi.mocked(signedRequest).mock.calls.map(call => call[1])).toEqual(["/v1/account/deletion-proof", "/v1/account/deletion-servers"]);
  expect(await completeDeletionGoogleReturn()).toBe(false);
});
it.each(["wrong-state", "account-switch"])("rejects %s before code exchange or provisioning", async failure => {
  if (failure === "wrong-state") window.history.replaceState({}, "", "/?code=controlled-code&state=wrong");
  else vi.mocked(loadCentralSession).mockReturnValue({ ...session, token: "replacement" });
  expect(await completeDeletionGoogleReturn()).toBe(true);
  expect(deletionGoogleReturnSnapshot()?.error).toBeTruthy();
  expect(signedRequest).not.toHaveBeenCalled();
  expect(sessionStorage.getItem("agentsassemble.accountDeletionGoogle.v1")).toBeNull();
});
it("retains confirmed progress while failing closed on a fresh inventory failure", async () => {
  vi.mocked(signedRequest).mockImplementation(async (_s, path) => {
    if (path.endsWith("deletion-proof")) return { request_id: requestId, proof: "p".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 };
    throw new Error("inventory unavailable");
  });
  await completeDeletionGoogleReturn();
  const result = deletionGoogleReturnSnapshot();
  expect(result?.operation?.progress).toEqual([]);
  expect(result?.operation?.inventoryReady).toBe(false);
  await expect(result?.operation?.disable("탈퇴")).rejects.toThrow("서버별");
});
