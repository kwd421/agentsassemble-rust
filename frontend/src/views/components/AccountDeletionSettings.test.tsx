import AccountDeletionSurface from "./AccountDeletionSurface";
import { EndedWorkspace } from "./CentralOwnerWorkspaceBoundary";
import { deletionOwnHost, stopDeletionOwnHost } from "../../lib/central/accountDeletionHost";
import { beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Launcher, { AccountDeletionFlow } from "./AccountDeletionSettings";
import LocalAccountDataChoice from "./LocalAccountDataChoice";
import { loadCentralSession, signedRequest, unsignedPost } from "../../lib/central/identity";
import { loadDeletionReceipt } from "../../lib/central/accountDeletion";
import { wipeDesktopAccountData } from "../../lib/desktopBridge";
vi.mock("../../lib/central/accountDeletionHost", () => ({ deletionOwnHost: vi.fn(async () => ({ host: null, hasOwnServers: false })), stopDeletionOwnHost: vi.fn() }));
vi.mock("../../lib/central/identity", () => ({ CENTRAL_SESSION_CHANGED_EVENT: "changed", CENTRAL_SESSION_CLEARED_EVENT: "cleared", centralAccountEntryUrl: () => "https://central.example/", loadCentralSession: vi.fn(), signedRequest: vi.fn(), unsignedPost: vi.fn(), clearCentralSession: vi.fn(), clearPendingCentralRecoveryCode: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ wipeDesktopAccountData: vi.fn() }));
const session = { token: "fixture-session", device_id: "device", expires_at: 2 ** 31, person: { person_id: "person", display_name: "Disposable fixture", identity_kind: "guest" as const } };
beforeEach(() => { cleanup(); vi.clearAllMocks(); sessionStorage.clear(); vi.mocked(loadCentralSession).mockReturnValue(session); });
it("shows hidden/skipped servers and requires explicit receipt lookup after a lost final response", async () => {
  const user = userEvent.setup();
  let disableCalls = 0;
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-servers")) return { servers: [{ server_id: "offline", registration_epoch: "epoch", name: "숨긴 오프라인 서버", user_hidden: true, host_state: "active", host_key_fingerprint: "", host_public_key_jwk: null, endpoint: null }] };
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 };
    if (method === "DELETE") { disableCalls++; throw new TypeError("최종 응답 연결 끊김"); }
    throw new Error("unexpected route");
  });
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  render(<AccountDeletionFlow disabled={false} />);
  expect(screen.queryByRole("list", { name: "서버별 탈퇴 정리 결과" })).toBeNull();
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect((await screen.findByRole("alert")).textContent).toContain("최종 응답");
  expect(screen.getByRole("list", { name: "서버별 탈퇴 정리 결과" }).textContent).toContain("지금 연결할 수 없어요.");
  expect(disableCalls).toBe(1); expect(unsignedPost).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(screen.getByLabelText("건너뛴 서버").textContent).toContain("숨긴 오프라인 서버");
  expect(screen.getByLabelText("건너뛴 서버").textContent).toContain("지금 연결할 수 없어요.");
  expect(disableCalls).toBe(1); expect(unsignedPost).toHaveBeenCalledOnce();
  expect(wipeDesktopAccountData).not.toHaveBeenCalled();
});
it("keeps local data by default and reports applied reset plus exact cleanup failure", async () => {
  const user = userEvent.setup();
  vi.mocked(wipeDesktopAccountData).mockResolvedValue({ reset: true, cleanup_errors: ["방 데이터는 지웠지만 캐시 정리에 실패했어요."] });
  render(<LocalAccountDataChoice disabled={false} />);
  expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
  expect(screen.queryByRole("button", { name: "이 컴퓨터의 방 데이터 삭제" })).toBeNull();
  expect(screen.getByText(/서버 키·호스팅 제한/).textContent).toContain("로그·그 밖의 파일은 유지돼요.");
  expect(wipeDesktopAccountData).not.toHaveBeenCalled();
  await user.click(screen.getByRole("checkbox"));
  await user.click(screen.getByRole("button", { name: "이 컴퓨터의 방 데이터 삭제" }));
  expect(wipeDesktopAccountData).toHaveBeenCalledOnce();
  expect((await screen.findByRole("status")).textContent).toContain("서버는 중지");
  expect(screen.getByRole("alert").textContent).toContain("캐시 정리에 실패");
});

it("keeps the separate startup choice when the retained result selects wipe", async () => {
  const user = userEvent.setup();
  render(<><LocalAccountDataChoice disabled={false} /><LocalAccountDataChoice disabled={false} /></>);
  const groups = screen.getAllByRole("group", { name: "이 컴퓨터의 방 데이터도 지울까요?" });
  await user.click(within(groups[0]).getByRole("checkbox"));
  expect((within(groups[1]).getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
  expect(within(groups[1]).queryByRole("button", { name: "이 컴퓨터의 방 데이터 삭제" })).toBeNull();
  expect(wipeDesktopAccountData).not.toHaveBeenCalled();
});

it("keeps the central account operation and optional local choice when its room/startup tree is replaced", async () => {
  const user = userEvent.setup();
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function(this: HTMLDialogElement) { this.open = true; });
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true });
  vi.mocked(stopDeletionOwnHost).mockResolvedValue();
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 };
    if (method === "DELETE") { await final; return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now() / 1000) + 86400 }; }
    throw new Error("unexpected route");
  });
  let release!: () => void;
  const final = new Promise<void>(resolve => { release = resolve; });
  const view = render(<AccountDeletionSurface><Launcher disabled={false} /></AccountDeletionSurface>);
  await user.click(screen.getByRole("button", { name: "계정 탈퇴" }));
  const dialog = screen.getByRole("dialog", { name: "계정 탈퇴" }), flow = within(dialog);
  await user.type(await flow.findByLabelText("복구 코드"), "fixture-input");
  await user.click(flow.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce();
  view.rerender(<AccountDeletionSurface><EndedWorkspace reason="disconnected" /></AccountDeletionSurface>);
  expect(screen.queryByRole("dialog", { name: "서버 연결 종료" })).toBeNull();
  release();
  expect(await flow.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect((flow.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
  expect(wipeDesktopAccountData).not.toHaveBeenCalled();
  await user.click(flow.getByRole("button", { name: "확인" }));
  expect(screen.getByRole("dialog", { name: "서버 연결 종료" })).toBeTruthy();
});

it("refreshes expired proof without repeating a confirmed own-host stop", async () => {
  const user = userEvent.setup();
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true });
  const realNow = Date.now.bind(Date);
  let elapsed = 0;
  vi.spyOn(Date, "now").mockImplementation(() => realNow() + elapsed);
  vi.mocked(stopDeletionOwnHost).mockImplementation(async () => { elapsed = 301_000; });
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-servers")) return { servers: [{ server_id: "offline", registration_epoch: "epoch", name: "꺼진 서버", user_hidden: false, host_state: "active", host_key_fingerprint: "", host_public_key_jwk: null, endpoint: null }] };
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000) + 300 };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000) + 86400 };
    throw new Error("unexpected route");
  });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce();
  vi.restoreAllMocks();
});

it("only checks a retained final receipt after remount with the same live account", async () => {
  const user = userEvent.setup();
  sessionStorage.setItem("agentsassemble.accountDeletionReceipt.v1", JSON.stringify({ person_id: session.person.person_id,
    request_id: "r".repeat(43), receipt: "s".repeat(43), expires_at: Math.floor(Date.now()/1000)+86400, results: [] }));
  vi.mocked(unsignedPost).mockRejectedValueOnce(new TypeError("결과 연결 끊김")).mockResolvedValueOnce({ status: "account_deleted" });
  const first = render(<AccountDeletionFlow disabled={false} />);
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  first.unmount();
  render(<AccountDeletionFlow disabled={false} />);
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(unsignedPost).toHaveBeenCalledTimes(2); expect(signedRequest).not.toHaveBeenCalled();
});

it("guides web owners to their server app without offering a native reset", async () => {
  const user = userEvent.setup();
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: null, hasOwnServers: true });
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 };
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    throw new Error("unexpected route");
  });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByText("방 데이터를 지우려면 서버 컴퓨터의 앱에서 선택해 주세요. 그대로 두어도 돼요.")).toBeTruthy();
  expect(screen.queryByRole("checkbox")).toBeNull(); expect(wipeDesktopAccountData).not.toHaveBeenCalled();
});

it("retains web owner guidance through a lost final response and receipt recovery after remount", async () => {
  const user = userEvent.setup();
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: null, hasOwnServers: true });
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 };
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (method === "DELETE") throw new TypeError("최종 응답 연결 끊김");
    throw new Error("unexpected route");
  });
  const first = render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  const calls = vi.mocked(signedRequest).mock.calls.length;
  first.unmount();
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  render(<AccountDeletionFlow disabled={false} />);
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByText("방 데이터를 지우려면 서버 컴퓨터의 앱에서 선택해 주세요. 그대로 두어도 돼요.")).toBeTruthy();
  expect(signedRequest).toHaveBeenCalledTimes(calls); expect(unsignedPost).toHaveBeenCalledOnce();
  expect(screen.queryByRole("checkbox")).toBeNull(); expect(wipeDesktopAccountData).not.toHaveBeenCalled();
});

it("retains the separate default-off local choice after native receipt recovery", async () => {
  const user = userEvent.setup();
  sessionStorage.setItem("agentsassemble.accountDeletionReceipt.v1", JSON.stringify({ person_id: session.person.person_id,
    request_id: "r".repeat(43), receipt: "s".repeat(43), expires_at: Math.floor(Date.now()/1000)+86400, results: [], local_followup: "local_stopped" }));
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  render(<AccountDeletionFlow disabled={false} />);
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
  expect(wipeDesktopAccountData).not.toHaveBeenCalled(); expect(signedRequest).not.toHaveBeenCalled();
});


it.each([[401, "account_deletion_reauth_required"], [429, "rate_limited"], [503, "abuse_limiter_unavailable"]])("reauthenticates after definite final rejection %s/%s without repeating confirmed host work", async (status, code) => {
  const user = userEvent.setup();
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true });
  vi.mocked(stopDeletionOwnHost).mockResolvedValue();
  let disableCalls = 0, proofCalls = 0;
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (path.endsWith("deletion-proof")) { proofCalls++; return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 }; }
    if (method === "DELETE") {
      disableCalls++;
      expect(loadDeletionReceipt()).not.toBeNull();
      if (disableCalls === 1) throw Object.assign(new Error("reauthenticate"), { status, code });
      return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    }
    throw new Error("unexpected route");
  });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect((await screen.findByRole("alert")).textContent).toContain("같은 계정으로 다시 확인");
  expect(loadDeletionReceipt()).toBeNull(); expect(loadCentralSession()).toEqual(session);
  expect(screen.getByRole("button", { name: "다시 시도" }).hasAttribute("disabled")).toBe(true);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(proofCalls).toBe(2); expect(disableCalls).toBe(2);
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce(); expect(unsignedPost).not.toHaveBeenCalled();
});

function retained(other = false, local_followup = "none") {
  sessionStorage.setItem("agentsassemble.accountDeletionReceipt.v1", JSON.stringify({ person_id: other ? "other-person" : session.person.person_id,
    request_id: "r".repeat(43), receipt: "s".repeat(43), expires_at: Math.floor(Date.now()/1000)+86400, results: [], local_followup }));
}
function stalledReceipt() {
  let started!: () => void, signal!: AbortSignal;
  const ready = new Promise<void>(resolve => { started = resolve; });
  vi.mocked(unsignedPost).mockImplementation((_path, _body, requestSignal) => new Promise((_resolve, reject) => {
    signal = requestSignal!; started(); signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
  }));
  return { ready, get signal() { return signal; } };
}
it.each(["계정 탈퇴 닫기", "취소"])("cancels stalled receipt lookup with %s without losing its receipt", async name => {
  retained(); const pending = stalledReceipt(), user = userEvent.setup();
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function(this: HTMLDialogElement) { this.open = true; });
  render(<AccountDeletionSurface><Launcher disabled={false} /></AccountDeletionSurface>);
  await user.click(screen.getByRole("button", { name: "계정 탈퇴" }));
  await user.click(screen.getByRole("button", { name: "다시 시도" })); await pending.ready;
  expect(screen.getByRole("button", { name }).hasAttribute("disabled")).toBe(false);
  await user.click(screen.getByRole("button", { name }));
  expect(pending.signal.aborted).toBe(true); expect(screen.queryByRole("dialog")).toBeNull();
  expect(loadDeletionReceipt()).not.toBeNull(); expect(signedRequest).not.toHaveBeenCalled(); vi.restoreAllMocks();
});
it.each(["deadline", "account-switch"])("aborts stalled receipt lookup on %s", async kind => {
  retained(); const pending = stalledReceipt(), expiry = new AbortController(), user = userEvent.setup();
  vi.spyOn(AbortSignal, "timeout").mockReturnValue(expiry.signal);
  render(<AccountDeletionFlow disabled={false} onClose={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "다시 시도" })); await pending.ready;
  if (kind === "deadline") expiry.abort(new DOMException("expired", "TimeoutError"));
  else { vi.mocked(loadCentralSession).mockReturnValue({ ...session, token: "other-token" }); window.dispatchEvent(new Event("changed")); }
  expect(await screen.findByRole("alert")).toBeTruthy(); expect(pending.signal.aborted).toBe(true);
  expect(loadDeletionReceipt()).not.toBeNull(); expect(signedRequest).not.toHaveBeenCalled(); vi.restoreAllMocks();
});
it.each([["local_stopped", false], ["server_app", false], ["local_stopped", true], ["server_app", true]] as const)("does not inherit another account's %s follow-up (recovered: %s)", async (followup, recovered) => {
  retained(true, followup); const user = userEvent.setup();
  if (recovered) vi.mocked(loadCentralSession).mockReturnValue(null);
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: null, hasOwnServers: false });
  vi.mocked(signedRequest).mockImplementation(async (_session, path, method, body) => {
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 };
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (method === "DELETE") return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    throw new Error("unexpected route");
  });
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  render(<AccountDeletionFlow disabled={false} />);
  if (recovered) {
    await user.click(screen.getByRole("button", { name: "다시 시도" }));
    await screen.findByRole("heading", { name: "탈퇴했어요" });
    act(() => { vi.mocked(loadCentralSession).mockReturnValue(session); window.dispatchEvent(new Event("changed")); });
    expect(screen.queryByRole("heading", { name: "탈퇴했어요" })).toBeNull();
    expect(loadDeletionReceipt()?.person_id).toBe("other-person");
  }
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(vi.mocked(signedRequest).mock.calls.filter(call => call[1].endsWith("deletion-proof"))).toHaveLength(1);
  expect(unsignedPost).toHaveBeenCalledTimes(recovered ? 1 : 0);
  expect(screen.queryByRole("checkbox")).toBeNull(); expect(screen.queryByText(/방 데이터를 지우려면 서버/)).toBeNull();
  expect(loadDeletionReceipt()).toMatchObject({ person_id: session.person.person_id, local_followup: "none" });
});


it.each(["invalid_session", "authentication_required", "invalid_signed_request"])("retries %s after same-account login with fresh authority and preserved host results", async code => {
  const user = userEvent.setup();
  const refreshed = { ...session, token: "refreshed-session", device_id: "refreshed-device" };
  vi.mocked(deletionOwnHost).mockResolvedValue({ host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true });
  vi.mocked(stopDeletionOwnHost).mockResolvedValue();
  let writes = 0, proofs = 0, inventories = 0;
  vi.mocked(signedRequest).mockImplementation(async (authority, path, method, body) => {
    if (path.endsWith("deletion-servers")) { inventories++; return { servers: [] }; }
    if (path.endsWith("deletion-proof")) { proofs++; return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 }; }
    if (method === "DELETE") {
      writes++;
      if (writes === 1) {
        vi.mocked(loadCentralSession).mockReturnValue(null);
        window.dispatchEvent(new Event("cleared"));
        throw Object.assign(new Error("다시 로그인해 주세요."), { status: 401, code });
      }
      expect(authority).toEqual(refreshed);
      return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    }
    throw new Error("unexpected route");
  });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  await screen.findByRole("alert");
  expect(loadCentralSession()).toBeNull(); expect(loadDeletionReceipt()).toBeNull();
  act(() => { vi.mocked(loadCentralSession).mockReturnValue(refreshed); window.dispatchEvent(new Event("changed")); });
  await user.type(screen.getByLabelText("복구 코드"), "fixture-input");
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(proofs).toBe(2); expect(writes).toBe(2); expect(inventories).toBe(1);
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce(); expect(unsignedPost).not.toHaveBeenCalled();
  expect(deletionOwnHost).toHaveBeenLastCalledWith(refreshed, expect.any(AbortSignal));
});


it("detaches A's ambiguous result when B retries in the same mounted dialog", async () => {
  const user = userEvent.setup();
  const other = { ...session, token: "other-session", person: { ...session.person, person_id: "other", display_name: "Other account" } };
  vi.mocked(deletionOwnHost).mockImplementation(async authority => authority.person.person_id === session.person.person_id
    ? { host: { server_id: "own", registration_epoch: "epoch", fingerprint: "fingerprint", key: "key" }, hasOwnServers: true }
    : { host: null, hasOwnServers: false });
  vi.mocked(stopDeletionOwnHost).mockResolvedValue();
  const writes: string[] = [], proofs: string[] = [];
  vi.mocked(signedRequest).mockImplementation(async (authority, path, method, body) => {
    const person = authority.person.person_id;
    if (path.endsWith("deletion-servers")) return { servers: person === session.person.person_id ? [{ server_id: "offline-A", registration_epoch: "epoch", name: "A server", user_hidden: false, host_state: "active", host_key_fingerprint: "", host_public_key_jwk: null, endpoint: null }] : [] };
    if (path.endsWith("deletion-proof")) { proofs.push(person); return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 }; }
    if (method === "DELETE") {
      writes.push(person);
      if (person === session.person.person_id) throw new TypeError("응답 연결 끊김");
      return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    }
    throw new Error("unexpected route");
  });
  vi.mocked(unsignedPost).mockResolvedValue({ status: "account_deleted" });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-A");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  await screen.findByRole("alert");
  expect(loadDeletionReceipt()?.person_id).toBe(session.person.person_id);
  act(() => { vi.mocked(loadCentralSession).mockReturnValue(other); window.dispatchEvent(new Event("changed")); });
  expect(screen.queryByText("A server")).toBeNull();
  expect(screen.getByText("Other account")).toBeTruthy();
  await user.type(screen.getByLabelText("복구 코드"), "fixture-B");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(proofs).toEqual([session.person.person_id, other.person.person_id]);
  expect(writes).toEqual([session.person.person_id, other.person.person_id]);
  expect(loadDeletionReceipt()?.person_id).toBe(other.person.person_id);
  expect(unsignedPost).not.toHaveBeenCalled(); expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.queryByLabelText("건너뛴 서버")).toBeNull();
  expect(stopDeletionOwnHost).toHaveBeenCalledOnce();
});


it.each(["resolve", "reject"])("ignores A's late %s after B logs in during deletion", async outcome => {
  const user = userEvent.setup(), other = { ...session, token: "other", person: { ...session.person, person_id: "other", display_name: "Other account" } };
  let complete!: (value: unknown) => void, fail!: (reason: Error) => void, finalBody: Record<string, unknown> | undefined;
  const pending = new Promise<unknown>((resolve, reject) => { complete = resolve; fail = reject; });
  const writes: string[] = [];
  vi.mocked(signedRequest).mockImplementation(async (authority, path, method, body) => {
    if (path.endsWith("deletion-proof")) return { request_id: body?.request_id, proof: "p".repeat(43), expires_at: Math.floor(Date.now()/1000)+300 };
    if (path.endsWith("deletion-servers")) return { servers: [] };
    if (method === "DELETE") {
      writes.push(authority.person.person_id);
      if (authority.token === session.token) { finalBody = body; return pending; }
      return { status: "account_deleted", request_id: body?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 };
    }
    throw new Error("unexpected route");
  });
  render(<AccountDeletionFlow disabled={false} />);
  await user.type(screen.getByLabelText("복구 코드"), "fixture-A");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(finalBody).toBeTruthy(); const receipt = loadDeletionReceipt();
  await act(async () => {
    vi.mocked(loadCentralSession).mockReturnValue(other); window.dispatchEvent(new Event("changed"));
    if (outcome === "reject") fail(new TypeError("이전 계정의 응답 연결 끊김"));
    else complete({ status: "account_deleted", request_id: finalBody?.request_id, receipt_expires_at: Math.floor(Date.now()/1000)+86400 });
    await pending.catch(() => {});
  });
  expect(loadDeletionReceipt()).toEqual(receipt); expect(loadCentralSession()).toEqual(other);
  expect(screen.queryByRole("heading", { name: "탈퇴했어요" })).toBeNull(); expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByRole("list", { name: "서버별 탈퇴 정리 결과" })).toBeNull(); expect(screen.queryByRole("checkbox")).toBeNull();
  await user.type(screen.getByLabelText("복구 코드"), "fixture-B");
  await user.click(screen.getByRole("button", { name: "복구 코드로 확인하고 탈퇴" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(writes).toEqual([session.person.person_id, other.person.person_id]); expect(unsignedPost).not.toHaveBeenCalled();
});
