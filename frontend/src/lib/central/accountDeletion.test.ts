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
it("keeps hidden/offline servers visible after fresh proof", async () => {
  const operation = await prepared();
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
  await operation.removeReachable(vi.fn());
  expect(operation.progress?.[0]).toMatchObject({ server: { user_hidden: true }, state: "skipped", reason: "지금 연결할 수 없어요." });
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
});
it("fails an incomplete list instead of disabling with an empty inventory", async () => {
  const operation = new AccountDeletion(session);
  vi.mocked(signedRequest).mockRejectedValue(new Error("list incomplete"));
  await expect(operation.inventory()).rejects.toThrow("list incomplete");
  await expect(operation.disable()).rejects.toThrow();
  expect(signedRequest).toHaveBeenCalledTimes(1);
  expect(() => parseDeletionServers({ servers: [offline, offline] })).toThrow();
});
it("cancels on account switch before any host or final dispatch", async () => {
  const operation = await prepared();
  vi.mocked(loadCentralSession).mockReturnValue({ ...session, token: "replacement" });
  await expect(operation.removeReachable(vi.fn())).rejects.toThrow("로그인 계정");
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
  expect(vi.mocked(signedRequest).mock.calls.some(c => c[2] === "DELETE")).toBe(false);
});
it.each([new TypeError("lost response"), Object.assign(new Error("server failure"), { status: 500, code: "internal_error" })])("retains one opaque receipt on ambiguous final failure: %s", async failure => {
  const operation = await prepared([]);
  await operation.removeReachable(vi.fn());
  vi.mocked(signedRequest).mockImplementation(async (_s, _p, _m, body) => {
    expect(loadDeletionReceipt()?.receipt).toBe(body?.receipt);
    throw failure;
  });
  await expect(operation.disable()).rejects.toThrow(failure.message);
  const receipt = loadDeletionReceipt();
  await expect(operation.disable()).rejects.toThrow(failure.message);
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
  await expect(operation.removeReachable(vi.fn())).rejects.toThrow();
  await expect(operation.disable()).rejects.toThrow();
  expect(RemoteTransport.connect).not.toHaveBeenCalled();
});

it.each(["wipe", { host_key: "fixture" }, true])("rejects invalid receipt follow-up before status lookup: %j", async local_followup => {
  sessionStorage.setItem("agentsassemble.accountDeletionReceipt.v1", JSON.stringify({ person_id: "person",
    request_id: "r".repeat(43), receipt: "s".repeat(43), expires_at: Math.floor(Date.now()/1000)+86400, results: [], local_followup }));
  await expect(checkDeletionReceipt()).rejects.toThrow();
  expect(unsignedPost).not.toHaveBeenCalled();
});

it.each(["removed", "new-epoch"])("retains confirmed removal from the first snapshot after proof expiry and membership %s", async mutation => {
  const op = new AccountDeletion(session), now = Date.now.bind(Date); let elapsed = 0, inventoryCalls = 0;
  vi.spyOn(Date, "now").mockImplementation(() => now()+elapsed);
  const hello = { protocol: "secure_admission_v1", client_public_key: "fixture-client", channel_id: "fixture-channel", origin: "https://host.example", generation: 1 };
  const server = { ...offline, server_id: "remote", name: "확정 서버", host_public_key_jwk: { kty: "OKP", crv: "Ed25519", x: "k".repeat(43) },
    endpoint: { origin: hello.origin, generation: 1, status: "published", mode: "event_secure_v1", protocol: hello.protocol, account_deletion_protocol: "v1" } };
  const remote = vi.fn(async (path: string) => Response.json(path.endsWith("challenge") ? { challenge_id: "fixture-challenge", challenge_hash: "c".repeat(43), server_id: server.server_id, registration_epoch: server.registration_epoch, expires_at: Math.floor(Date.now()/1000)+300 } : { status: "account_removed" }));
  vi.mocked(RemoteTransport.connect).mockResolvedValue({ hello, fetch: remote, close: vi.fn() } as never);
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    const expires_at = Math.floor(Date.now()/1000)+300;
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at };
    if (path.endsWith("deletion-servers")) { inventoryCalls++; return { servers: inventoryCalls === 1 ? [server] : mutation === "removed" ? [] : [{ ...server, registration_epoch: "replacement" }] }; }
    if (path.endsWith("account-deletion-grants")) return { protocol: hello.protocol, client_public_key: hello.client_public_key, channel_id: hello.channel_id, purpose: "account_deletion", request_id: op.requestId, server_id: server.server_id, registration_epoch: server.registration_epoch, endpoint_origin: hello.origin, endpoint_generation: 1, grant_token: "aadg1."+"g".repeat(43), expires_at };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    throw new Error("unexpected route");
  });
  try {
    await op.confirmGuest("fixture-input"); await op.inventory(); await op.removeReachable(vi.fn());
    expect(op.progress?.[0].state).toBe("removed"); elapsed = 301_000;
    await expect(op.disable()).rejects.toThrow("확인 시간");
    await op.confirmGuest("fixture-input"); await op.inventory(); await op.removeReachable(vi.fn()); await op.disable();
    expect(inventoryCalls).toBe(1); expect(remote).toHaveBeenCalledTimes(2);
    expect(loadDeletionReceipt()?.results).toMatchObject([{ server_id: "remote", registration_epoch: "epoch", state: "removed" }]);
  } finally { op.cancel(); vi.restoreAllMocks(); }
});
