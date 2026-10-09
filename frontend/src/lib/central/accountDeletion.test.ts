import { beforeEach, expect, it, vi } from "vitest";
import { AccountDeletion, checkDeletionReceipt, loadDeletionReceipt, parseDeletionServers } from "./accountDeletion";
import { loadCentralSession, signedRequest, unsignedPost } from "./identity";
vi.mock("./accountDeletionHost", () => ({ deletionOwnHost: vi.fn(async () => ({ host: null, hasOwnServers: false })), stopDeletionOwnHost: vi.fn() }));
import { RemoteTransport } from "../remote/remoteTransport";
vi.mock("./identity", () => ({ CENTRAL_SESSION_CHANGED_EVENT: "changed", CENTRAL_SESSION_CLEARED_EVENT: "cleared", loadCentralSession: vi.fn(), signedRequest: vi.fn(), unsignedPost: vi.fn(), clearCentralSession: vi.fn(), clearPendingCentralRecoveryCode: vi.fn() }));
vi.mock("../remote/remoteTransport", () => ({ RemoteTransport: { connect: vi.fn() } }));
const session = { token: "session", device_id: "device", expires_at: 2 ** 31, person: { person_id: "person", display_name: "Guest", identity_kind: "guest" as const } };
const offline = { server_id: "offline", registration_epoch: "epoch", name: "꺼진 서버", user_hidden: true, host_state: "active", host_key_fingerprint: "", host_public_key_jwk: null, endpoint: null };
beforeEach(() => { vi.clearAllMocks(); sessionStorage.clear(); vi.mocked(loadCentralSession).mockReturnValue(session); });
async function prepared(servers = [offline]) {
  const operation = new AccountDeletion(session);
  vi.mocked(signedRequest).mockImplementation(async (_s, path) => path.endsWith("deletion-proof") ? { request_id: operation.requestId, proof: "p".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 } : { servers });
  await operation.confirmGuest("existing code"); await operation.inventory();
  return operation;
}
it("keeps hidden/offline servers visible and requires typed confirmation before side effects", async () => {
  const operation = await prepared();
  await expect(operation.removeReachable("", vi.fn())).rejects.toThrow();
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
  await operation.removeReachable("탈퇴", vi.fn());
  expect(operation.progress?.[0]).toMatchObject({ server: { user_hidden: true }, state: "skipped", reason: "지금 연결할 수 없어요." });
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
});
it("fails an incomplete list instead of disabling with an empty inventory", async () => {
  const operation = new AccountDeletion(session);
  vi.mocked(signedRequest).mockRejectedValue(new Error("list incomplete"));
  await expect(operation.inventory()).rejects.toThrow("list incomplete");
  await expect(operation.disable("탈퇴")).rejects.toThrow();
  expect(signedRequest).toHaveBeenCalledTimes(1);
  expect(() => parseDeletionServers({ servers: [offline, offline] })).toThrow();
});
it("cancels on account switch before any host or final dispatch", async () => {
  const operation = await prepared();
  vi.mocked(loadCentralSession).mockReturnValue({ ...session, token: "replacement" });
  await expect(operation.removeReachable("탈퇴", vi.fn())).rejects.toThrow("로그인 계정");
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
  expect(vi.mocked(signedRequest).mock.calls.some(c => c[2] === "DELETE")).toBe(false);
});
it("stores exactly one opaque receipt before a lost disable and checks it explicitly without login", async () => {
  const operation = await prepared([]);
  await operation.removeReachable("탈퇴", vi.fn());
  vi.mocked(signedRequest).mockImplementation(async (_s, _p, _m, body) => {
    expect(loadDeletionReceipt()?.receipt).toBe(body?.receipt);
    throw new TypeError("lost response");
  });
  await expect(operation.disable("탈퇴")).rejects.toThrow("lost response");
  const receipt = loadDeletionReceipt();
  await expect(operation.disable("탈퇴")).rejects.toThrow("lost response");
  expect(loadDeletionReceipt()).toEqual(receipt);
  vi.mocked(loadCentralSession).mockReturnValue(null);
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  expect(await checkDeletionReceipt()).toBe("account_deleted");
  expect(unsignedPost).toHaveBeenCalledOnce();
  expect(vi.mocked(unsignedPost).mock.calls[0][1]).toEqual({ person_id: receipt?.person_id, receipt: receipt?.receipt });
  expect(JSON.stringify(receipt)).not.toContain("session");
});

it("never uses restored progress as a fresh inventory after a list failure", async () => {
  const operation = new AccountDeletion(session);
  operation.progress = [{ server: offline, state: "removed" }];
  vi.mocked(signedRequest).mockRejectedValue(new Error("list unavailable"));
  await expect(operation.inventory()).rejects.toThrow();
  await expect(operation.removeReachable("탈퇴", vi.fn())).rejects.toThrow();
  await expect(operation.disable("탈퇴")).rejects.toThrow();
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
});
