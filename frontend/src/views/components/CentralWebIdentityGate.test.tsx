import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import CentralWebIdentityGate from "./CentralWebIdentityGate";

const mocks = vi.hoisted(() => ({ bootstrap: vi.fn(), open: vi.fn(), logout: vi.fn(), prepare: vi.fn(),
  session: null as object | null, callback: null as ((response: { credential: string }) => void) | null }));
vi.mock("../../lib/centralIdentity", () => ({ bootstrapCentral: mocks.bootstrap, loadCentralSession: () => mocks.session,
  logoutCentral: mocks.logout, openCentralOwnedServer: mocks.open }));
vi.mock("../../lib/centralWebGoogle", () => ({ prepareCentralWebGoogle: mocks.prepare }));
vi.mock("../../lib/googleIdentity", () => ({ loadGoogleIdentityScript: async () => {}, googleIdentityApi: () => ({
  initialize: ({ callback }: { callback: typeof mocks.callback }) => { mocks.callback = callback; },
  renderButton: () => {}, cancel: () => {},
}) }));
const account = { person: { display_name: "Existing Google User" }, servers: [
  { server_id: "online", alias: "My Mac", relation: "owner", endpoint: { status: "likely_online", lease_expires_at: 9_999_999_999 } },
  { server_id: "offline", alias: "My Windows", relation: "owner", endpoint: null },
  { server_id: "bookmark", alias: "Invited server", relation: "bookmark", endpoint: { status: "likely_online", lease_expires_at: 9_999_999_999 } },
] };
afterEach(() => { cleanup(); vi.resetAllMocks(); mocks.session = null; mocks.callback = null; });

it("requires live validation and exposes an owner grant failure without entering a room", async () => {
  mocks.session = {};
  mocks.bootstrap.mockRejectedValueOnce(new Error("session revoked")).mockResolvedValueOnce(account);
  mocks.open.mockRejectedValue(new Error("host went offline"));
  render(<CentralWebIdentityGate />);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "session revoked");
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

it("ignores a cancelled Google result and lists servers only after completion and validation", async () => {
  mocks.bootstrap.mockResolvedValueOnce(null).mockResolvedValueOnce(account);
  const complete = vi.fn().mockResolvedValue({});
  mocks.prepare.mockResolvedValue({ clientId: "web", nonce: "nonce", complete });
  render(<CentralWebIdentityGate />);
  await act(async () => { await Promise.resolve(); });
  fireEvent.click(screen.getByRole("button", { name: "Google로 계속" }));
  fireEvent.click(await screen.findByRole("button", { name: "로그인 취소" }));
  await act(async () => { mocks.callback?.({ credential: "cancelled-result" }); });
  expect(screen.queryByText("My Mac")).toBeNull();
  expect(complete).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Google로 계속" }));
  await screen.findByRole("button", { name: "로그인 취소" });
  await act(async () => { mocks.callback?.({ credential: "verified-result" }); });
  expect(await screen.findByText("My Mac")).toBeTruthy();
});
