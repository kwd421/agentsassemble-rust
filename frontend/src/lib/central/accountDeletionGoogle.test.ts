import { beforeEach, expect, it, vi } from "vitest";
import { AccountDeletion } from "./accountDeletion";
import { confirmDeletionGoogle } from "./accountDeletionGoogle";
import { loadCentralSession, signedRequest } from "./identity";
import { authorizeDeletionGooglePopup } from "./deletionGooglePopup";
import { nativeGoogleAuthorization } from "./nativeGoogleAuthorization";
import { isDesktopWebview } from "../desktopBridge";
vi.mock("./identity", () => ({ CENTRAL_SESSION_CHANGED_EVENT: "changed", CENTRAL_SESSION_CLEARED_EVENT: "cleared", isCentralWebEntry: () => true, loadCentralSession: vi.fn(), signedRequest: vi.fn(), parseCentralGoogleHandoff: vi.fn() }));
vi.mock("./deletionGooglePopup", () => ({ authorizeDeletionGooglePopup: vi.fn() }));
vi.mock("./nativeGoogleAuthorization", () => ({ nativeGoogleAuthorization: vi.fn() }));
vi.mock("../desktopBridge", () => ({ isDesktopWebview: vi.fn() }));
const session = { token: "fixture-session", device_id: "device", expires_at: 2 ** 31, person: { person_id: "person", display_name: "Google fixture", identity_kind: "google" as const } };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(loadCentralSession).mockReturnValue(session); });
it.each([false, true])("%s native: accepts only original-operation proof after reauth, without login or cleanup", async native => {
  vi.mocked(isDesktopWebview).mockReturnValue(native);
  const op = new AccountDeletion(session);
  const body = { authorization_code: "fixture-code", code_verifier: "v".repeat(43), handoff_id: `goh_${op.requestId}` };
  vi.mocked(authorizeDeletionGooglePopup).mockResolvedValue(body);
  vi.mocked(nativeGoogleAuthorization).mockResolvedValue({ body, expiresAt: Math.floor(Date.now()/1000) + 300 });
  vi.mocked(signedRequest).mockResolvedValue({ request_id: op.requestId, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000) + 300 });
  await confirmDeletionGoogle(op, new AbortController().signal);
  expect(op.hasFreshProof).toBe(true); expect(op.inventoryReady).toBe(false);
  expect(signedRequest).toHaveBeenCalledOnce();
  expect(vi.mocked(signedRequest).mock.calls[0][3]).toMatchObject({ flow_kind: native ? "native" : "web", action: "complete", request_id: op.requestId });
  op.cancel();
});
it("rejects account switch after callback before proof dispatch", async () => {
  vi.mocked(isDesktopWebview).mockReturnValue(false);
  const op = new AccountDeletion(session);
  vi.mocked(authorizeDeletionGooglePopup).mockImplementation(async () => {
    vi.mocked(loadCentralSession).mockReturnValue({ ...session, token: "replacement" });
    return { authorization_code: "fixture-code", code_verifier: "v".repeat(43) };
  });
  await expect(confirmDeletionGoogle(op, new AbortController().signal)).rejects.toThrow();
  expect(signedRequest).not.toHaveBeenCalled(); expect(op.hasFreshProof).toBe(false); op.cancel();
});
