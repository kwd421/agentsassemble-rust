import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import AccountDeletionSurface from "./AccountDeletionSurface";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import StartupIdentityGate from "./StartupIdentityGate";

const mocks = vi.hoisted(() => ({ googleReturn: vi.fn(), bootstrap: vi.fn(), open: vi.fn(), openMember: vi.fn(), logout: vi.fn(), prepare: vi.fn(), receipt: vi.fn(),
  session: null as object | null, callback: null as ((response: { credential: string }) => void) | null }));
vi.mock("../../lib/central/identity", () => ({
  hasPendingLocalDemotion: vi.fn(() => false), retryPendingLocalDemotion: vi.fn(),
  CENTRAL_SESSION_CHANGED_EVENT: "agentsassemble:central-session-changed",
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => true, isCentralWebEntry: () => true,
  centralAccountEntryUrl: () => "https://central.example/",
  centralSessionLoggedOut: () => false, loadPendingCentralRecoveryCode: () => "",
  bootstrapCentral: mocks.bootstrap, loadCentralSession: () => mocks.session,
  isCentralAuthenticationError: () => false,
  logoutCentral: mocks.logout, openCentralOwnedServer: mocks.open, openCentralMemberServer: mocks.openMember,
  unsignedPost: mocks.receipt, clearCentralSession: vi.fn(), clearPendingCentralRecoveryCode: vi.fn(),
}));
vi.mock("../../lib/central/webGoogle", () => ({
  startCentralWebGoogle: mocks.prepare, completeCentralWebGoogleReturn: mocks.googleReturn,
}));
vi.mock("../../lib/desktopBridge", () => ({
  isBundledDesktopWebview: () => false,
  isDesktopWebview: () => false,
  requestDesktopServerRestore: vi.fn().mockResolvedValue(null),
  requestDesktopHostProductSurface: () => { throw new Error("browser called native host"); },
}));
const completeStartup = () => {};
const account = { person: { display_name: "Existing Google User" }, servers: [
  { server_id: "online", alias: "My Mac", relation: "owner", endpoint: { mode: "event_secure_v1", protocol: "secure_admission_v1", status: "published", lease_expires_at: 9_999_999_999 } },
  { server_id: "offline", alias: "My Windows", relation: "owner", endpoint: null },
  { server_id: "bookmark", alias: "Invited server", relation: "bookmark", endpoint: { mode: "event_secure_v1", protocol: "secure_admission_v1", status: "published", lease_expires_at: 9_999_999_999 } },
] };
afterEach(() => { cleanup(); sessionStorage.clear(); vi.resetAllMocks(); mocks.session = null; mocks.callback = null; });

it("opens shared account deletion settings without first opening a room", async () => {
  mocks.session = { person: { display_name: "Disposable Guest", identity_kind: "guest" } };
  mocks.bootstrap.mockResolvedValue({ ...account, person: { display_name: "Disposable Guest" } });
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  fireEvent.click(await screen.findByRole("button", { name: "계정 설정" }));
  expect(screen.getByRole("button", { name: "계정 탈퇴" })).toBeTruthy();
  expect(screen.getAllByRole("button", { name: "로그아웃" })).toHaveLength(1);
  expect(screen.queryByRole("button", { name: "My Mac 서버 열기" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "← 돌아가기" }));
  expect(await screen.findByRole("button", { name: "My Mac 서버 열기" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "계정 탈퇴" })).toBeNull();
  expect(mocks.open).not.toHaveBeenCalled();
});

it("requires live validation and exposes an owner grant failure without entering a room", async () => {
  mocks.session = {};
  mocks.bootstrap.mockRejectedValueOnce(new Error("session revoked")).mockResolvedValue(account);
  mocks.open.mockRejectedValue(new Error("host went offline"));
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  expect((await screen.findByRole("alert")).textContent).toContain("session revoked");
  expect(screen.queryByText("My Mac")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  const open = await screen.findByRole("button", { name: "My Mac 서버 열기" });
  expect(screen.getByRole("button", { name: "계정 설정" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "My Windows 서버 열기" })).toHaveProperty("disabled", true);
  expect(screen.getByRole("button", { name: "Invited server 서버 열기" })).toHaveProperty("disabled", true);
  fireEvent.click(open);
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("서버를 열지 못했어요.");
  expect(alert.querySelector("details")?.textContent).toContain("host went offline");
  expect(alert.querySelector("details")?.open).toBe(false);
  mocks.logout.mockResolvedValue(undefined);
  fireEvent.click(screen.getByRole("button", { name: "로그아웃" }));
  await screen.findByRole("button", { name: "Google로 계속" });
  expect(screen.queryByText("My Mac")).toBeNull();
});

it("keeps cancellation and retry on the common startup screen until Google returns", async () => {
  const cancelled = new DOMException("cancelled", "AbortError");
  mocks.prepare.mockRejectedValueOnce(cancelled).mockResolvedValueOnce(undefined);
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google로 계속" }));
  expect((await screen.findByRole("alert")).textContent).toContain("취소");
  expect(screen.queryByText("My Mac")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Google로 계속" }));
  await act(async () => { await Promise.resolve(); });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("My Mac")).toBeNull();
});

it("reaches a retained receipt from ordinary signed-out startup without login or a deep link", async () => {
  window.history.replaceState({}, "", "/");
  sessionStorage.setItem("agentsassemble.accountDeletionReceipt.v1", JSON.stringify({
    person_id: "deleted-fixture", request_id: "r".repeat(43), receipt: "s".repeat(43),
    expires_at: Math.floor(Date.now() / 1000) + 86400, results: [],
  }));
  mocks.receipt.mockResolvedValue({ status: "account_deleted" });
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function(this: HTMLDialogElement) { this.open = true; });
  render(<AccountDeletionSurface><StartupIdentityGate deviceToken="" onComplete={completeStartup} /></AccountDeletionSurface>);
  fireEvent.click(await screen.findByRole("button", { name: "계정 탈퇴" }));
  fireEvent.click(screen.getByRole("button", { name: "계정 탈퇴" }));
  expect(mocks.receipt).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByRole("heading", { name: "탈퇴했어요" })).toBeTruthy();
  expect(mocks.receipt).toHaveBeenCalledExactlyOnceWith("/v1/account-deletions/" + "r".repeat(43) + "/status", { person_id: "deleted-fixture", receipt: "s".repeat(43) }, expect.any(AbortSignal));
  expect(mocks.bootstrap).not.toHaveBeenCalled();
  expect(mocks.prepare).not.toHaveBeenCalled();
  expect(mocks.open).not.toHaveBeenCalled();
});

it.each(["return", "login"])("shows a dedicated deleted-account card from Google %s without startup failure or implicit signup", async source => {
  const request = new GoogleRegistrationRequired("deleted", "web", {} as never, 9999999999, null);
  const register = vi.spyOn(request, "register");
  if (source === "return") mocks.googleReturn.mockRejectedValueOnce(request);
  else mocks.prepare.mockRejectedValueOnce(request);
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  if (source === "login") fireEvent.click(await screen.findByRole("button", { name: "Google로 계속" }));
  expect(await screen.findByRole("heading", { name: "탈퇴한 계정이에요" })).toBeTruthy();
  expect(screen.getByText("이 Google 계정으로 새로 가입하면 예전 서버와 방은 이어지지 않아요.")).toBeTruthy();
  expect(screen.getAllByRole("button").map(button => button.textContent)).toEqual(["새 계정 만들기", "다른 계정으로 로그인"]);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("앱을 시작하지 못했어요.")).toBeNull();
  expect(register).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "다른 계정으로 로그인" }));
  expect(await screen.findByRole("button", { name: "Google로 계속" })).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "탈퇴한 계정이에요" })).toBeNull();
  expect(register).not.toHaveBeenCalled();
});


it.each(["guest", "google"])("offers the appropriate empty directory guidance for %s", async identity_kind => {
  const person = { person_id: `empty-${identity_kind}`, display_name: "테스트", identity_kind };
  mocks.session = { person };
  mocks.bootstrap.mockResolvedValue({ person, servers: [] });
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  expect(await screen.findByText(identity_kind === "guest"
    ? "초대 링크를 받으면 여기에서 바로 들어갈 수 있어요."
    : "아직 내 서버가 없어요. AgentsAssemble 앱을 설치한 컴퓨터에서 같은 계정으로 로그인하면 서버가 생겨요.")).toBeTruthy();
  expect(mocks.open).not.toHaveBeenCalled();
});


it("shows a guest's member servers and opens them through member admission", async () => {
  const person = { person_id: "member-guest", display_name: "참가자", identity_kind: "guest" };
  const server = { ...account.servers[0], relation: "member", alias: "초대받은 서버" };
  mocks.session = { person };
  mocks.bootstrap.mockResolvedValue({ person, servers: [server] });
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  fireEvent.click(await screen.findByRole("button", { name: "초대받은 서버 서버 열기" }));
  await act(async () => { await Promise.resolve(); });
  expect(mocks.openMember).toHaveBeenCalledWith(server);
  expect(mocks.open).not.toHaveBeenCalled();
  expect(screen.queryByText(/아직 내 서버가 없어요/)).toBeNull();
});
