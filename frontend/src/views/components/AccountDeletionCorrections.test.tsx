import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Launcher, { AccountDeletionFlow } from "./AccountDeletionSettings";
import AccountDeletionSurface from "./AccountDeletionSurface";
import { clearExpiredDeletionReceipt, loadDeletionReceipt } from "../../lib/central/accountDeletion";
import { loadCentralSession, signedRequest, unsignedPost } from "../../lib/central/identity";
import { deletionOwnHost, stopDeletionOwnHost } from "../../lib/central/accountDeletionHost";
import { RemoteTransport } from "../../lib/remote/remoteTransport";
vi.mock("../../lib/central/identity", () => ({ CENTRAL_SESSION_CHANGED_EVENT: "changed", CENTRAL_SESSION_CLEARED_EVENT: "cleared", loadCentralSession: vi.fn(), signedRequest: vi.fn(), unsignedPost: vi.fn(), clearCentralSession: vi.fn(), clearPendingCentralRecoveryCode: vi.fn() }));
vi.mock("../../lib/central/accountDeletionHost", () => ({ deletionOwnHost: vi.fn(), stopDeletionOwnHost: vi.fn() }));
vi.mock("../../lib/remote/remoteTransport", () => ({ RemoteTransport: { connect: vi.fn() } }));
const session = { token: "fixture", device_id: "device", expires_at: 2 ** 31, person: { person_id: "person", display_name: "Fixture", identity_kind: "guest" as const } };
const key = "agentsassemble.accountDeletionReceipt.v1";
const expired = () => ({ person_id: "person", request_id: "r".repeat(43), receipt: "s".repeat(43), expires_at: Math.floor(Date.now()/1000)-1, results: [] });
beforeEach(() => { vi.clearAllMocks(); sessionStorage.clear(); vi.mocked(loadCentralSession).mockReturnValue(session); vi.mocked(deletionOwnHost).mockResolvedValue({ host: null, hasOwnServers: false }); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });
it.each(["계정 탈퇴 닫기", "취소"])("aborts deferred removal via %s and resumes without repeating confirmed servers", async control => {
  const user = userEvent.setup();
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function(this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function(this: HTMLDialogElement) { this.open = false; });
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true });
  vi.mocked(stopDeletionOwnHost).mockResolvedValue();
  const hello = { protocol: "secure_admission_v1", client_public_key: "client", channel_id: "channel", origin: "https://host.example", generation: 1 };
  const servers = ["first", "pending"].map(server_id => ({ server_id, registration_epoch: "epoch", name: server_id, user_hidden: false, host_state: "active", host_key_fingerprint: "fingerprint", host_public_key_jwk: { kty: "OKP", crv: "Ed25519", x: "k".repeat(43) }, endpoint: { ...hello, status: "published", mode: "event_secure_v1", account_deletion_protocol: "v1" } }));
  // Only wire endpoint fields are part of the inventory contract.
  const inventory = servers.map(server => ({ ...server, endpoint: { origin: hello.origin, generation: 1, status: "published", mode: "event_secure_v1", protocol: hello.protocol, account_deletion_protocol: "v1" } }));
  let blocked!: AbortSignal;
  let announce!: () => void;
  const pending = new Promise<void>(resolve => { announce = resolve; });
  let pause = true;
  const removed: string[] = [];
  vi.mocked(RemoteTransport.connect).mockImplementation(async (target, _purpose, signal) => {
    if (target.server_id === "pending" && pause) {
      pause = false; blocked = signal!; announce();
      await new Promise<void>((_resolve, reject) => signal!.addEventListener("abort", () => reject(signal!.reason), { once: true }));
    }
    return { hello, close: vi.fn(), fetch: async (path: string) => {
      if (path.endsWith("challenge")) return Response.json({ challenge_id: "challenge", challenge_hash: "c".repeat(43), server_id: target.server_id, registration_epoch: "epoch", expires_at: Math.floor(Date.now()/1000)+300 });
      removed.push(target.server_id); return Response.json({ status: "account_removed" });
    } } as never;
  });
  vi.mocked(signedRequest).mockImplementation(async (_s, path, method, body) => {
    const expires_at = Math.floor(Date.now()/1000)+300;
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at };
    if (path.endsWith("deletion-servers")) return { servers: inventory };
    if (path.endsWith("account-deletion-grants")) return { protocol: hello.protocol, client_public_key: hello.client_public_key, channel_id: hello.channel_id, purpose: "account_deletion", request_id: body?.request_id, server_id: path.split("/")[3], registration_epoch: "epoch", endpoint_origin: hello.origin, endpoint_generation: 1, grant_token: "aadg1."+"g".repeat(43), expires_at };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    throw new Error("unexpected route");
  });
  render(<AccountDeletionSurface><Launcher disabled={false} /></AccountDeletionSurface>);
  await user.click(screen.getByRole("button", { name: "계정 탈퇴" }));
  await user.type(screen.getByLabelText("복구 코드"), "fixture-code");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  await act(async () => { await pending; });
  expect(removed).toEqual(["first"]);
  expect((screen.getByRole("button", { name: control }) as HTMLButtonElement).disabled).toBe(false);
  await user.click(screen.getByRole("button", { name: control }));
  expect(blocked.aborted).toBe(true);
  expect(screen.queryByRole("dialog", { name: "계정 탈퇴" })).toBeNull();
  expect(vi.mocked(signedRequest).mock.calls.filter(c => c[2] === "DELETE")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "계정 탈퇴" }));
  const flow = within(screen.getByRole("dialog", { name: "계정 탈퇴" }));
  expect(flow.getByRole("list", { name: "서버별 탈퇴 정리 결과" }).textContent).toContain("완료");
  await user.type(flow.getByLabelText("복구 코드"), "fixture-code");
  await user.click(flow.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await flow.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(removed).toEqual(["first", "pending"]);
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce();
  expect(vi.mocked(signedRequest).mock.calls.filter(c => c[1].endsWith("deletion-proof"))).toHaveLength(2);
  expect(vi.mocked(signedRequest).mock.calls.filter(c => c[1].endsWith("deletion-servers"))).toHaveLength(1);
});
it.each([false, true])("starts a fresh deletion after a receipt expires (already mounted: %s)", async mounted => {
  const user = userEvent.setup();
  const receipt = expired();
  if (mounted) receipt.expires_at += 2;
  sessionStorage.setItem(key, JSON.stringify(receipt));
  render(<AccountDeletionFlow disabled={false} />);
  if (mounted) vi.spyOn(Date, "now").mockReturnValue((receipt.expires_at+1)*1000);
  vi.mocked(signedRequest).mockImplementation(async (_s, path, method, body) => {
    expect(body?.request_id).not.toBe(receipt.request_id);
    if (path.endsWith("deletion-proof")) { expect(loadDeletionReceipt()).toBeNull(); return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 }; }
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    throw new Error("unexpected route");
  });
  await user.type(screen.getByLabelText("복구 코드"), "fixture-code");
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(unsignedPost).not.toHaveBeenCalled();
  expect(loadDeletionReceipt()?.request_id).not.toBe(receipt.request_id);
  expect(vi.mocked(signedRequest).mock.calls.filter(c => c[2] === "DELETE")).toHaveLength(1);
});
it("removes only the exact expired receipt and preserves a replacement and unrelated storage", () => {
  const receipt = expired();
  sessionStorage.setItem(key, JSON.stringify(receipt)); sessionStorage.setItem("unrelated", "keep");
  const replacement = { ...receipt, receipt: "t".repeat(43) };
  sessionStorage.setItem(key, JSON.stringify(replacement)); clearExpiredDeletionReceipt(receipt);
  expect(loadDeletionReceipt()).toEqual(replacement);
  clearExpiredDeletionReceipt(replacement); expect(loadDeletionReceipt()).toBeNull();
  expect(sessionStorage.getItem("unrelated")).toBe("keep");
});
