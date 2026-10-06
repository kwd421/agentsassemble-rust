import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import StartupIdentityGate from "./StartupIdentityGate";

const mocks = vi.hoisted(() => ({ bootstrap: vi.fn(), open: vi.fn(), logout: vi.fn(), prepare: vi.fn(),
  session: null as object | null, callback: null as ((response: { credential: string }) => void) | null }));
vi.mock("../../lib/central/identity", () => ({
  hasPendingLocalDemotion: vi.fn(() => false), retryPendingLocalDemotion: vi.fn(),
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => true, isCentralWebEntry: () => true,
  centralSessionLoggedOut: () => false, loadPendingCentralRecoveryCode: () => "",
  bootstrapCentral: mocks.bootstrap, loadCentralSession: () => mocks.session,
  isCentralAuthenticationError: () => false,
  logoutCentral: mocks.logout, openCentralOwnedServer: mocks.open,
}));
vi.mock("../../lib/central/webGoogle", () => ({
  startCentralWebGoogle: mocks.prepare, completeCentralWebGoogleReturn: async () => {},
}));
vi.mock("../../lib/desktopBridge", () => ({
  requestDesktopHostProductSurface: () => { throw new Error("browser called native host"); },
}));
const completeStartup = () => {};
const account = { person: { display_name: "Existing Google User" }, servers: [
  { server_id: "online", alias: "My Mac", relation: "owner", endpoint: { status: "likely_online", lease_expires_at: 9_999_999_999 } },
  { server_id: "offline", alias: "My Windows", relation: "owner", endpoint: null },
  { server_id: "bookmark", alias: "Invited server", relation: "bookmark", endpoint: { status: "likely_online", lease_expires_at: 9_999_999_999 } },
] };
afterEach(() => { cleanup(); vi.resetAllMocks(); mocks.session = null; mocks.callback = null; });

it("requires live validation and exposes an owner grant failure without entering a room", async () => {
  mocks.session = {};
  mocks.bootstrap.mockRejectedValueOnce(new Error("session revoked")).mockResolvedValue(account);
  mocks.open.mockRejectedValue(new Error("host went offline"));
  render(<StartupIdentityGate deviceToken="" onComplete={completeStartup} />);
  expect((await screen.findByRole("alert")).textContent).toContain("session revoked");
  expect(screen.queryByText("My Mac")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  const open = await screen.findByRole("button", { name: "My Mac 서버 열기" });
  expect(screen.getByRole("button", { name: "My Windows 서버 열기" })).toHaveProperty("disabled", true);
  expect(screen.getByRole("button", { name: "Invited server 서버 열기" })).toHaveProperty("disabled", true);
  fireEvent.click(open);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "host went offline");
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
