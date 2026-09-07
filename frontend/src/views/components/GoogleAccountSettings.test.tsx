import "../../test/nativeDialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import GoogleAccountSettings from "./GoogleAccountSettings";

const mocks = vi.hoisted(() => ({
  status: vi.fn(), start: vi.fn(), connect: vi.fn(), disconnect: vi.fn(), desktop: vi.fn(),
  load: vi.fn(), initialize: vi.fn(), render: vi.fn(), cancel: vi.fn(),
}));
vi.mock("../../api/accounts", () => ({ fetchAccountStatus: mocks.status, startGoogleAccountLogin: mocks.start, connectGoogleAccount: mocks.connect, disconnectGoogleAccount: mocks.disconnect }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: mocks.desktop }));
vi.mock("../../lib/googleIdentity", () => ({ loadGoogleIdentityScript: mocks.load, googleIdentityApi: () => ({ initialize: mocks.initialize, renderButton: mocks.render, cancel: mocks.cancel }) }));
afterEach(cleanup);
const account = { account_id: "account", provider: "google", display_name: "", email: "", avatar_image_url: "" };
const identity = { deviceToken: "fixture-device", sessionToken: "fixture-session" };
beforeEach(() => {
  vi.clearAllMocks();
  mocks.desktop.mockReturnValue(false);
  mocks.status.mockResolvedValue({ account: null, google: { enabled: true, client_id: "fixture-client", unavailable_reason: "" } });
  mocks.start.mockResolvedValue({ status: "ready", client_id: "fixture-client", nonce: "nonce" });
  mocks.load.mockResolvedValue(undefined);
  mocks.connect.mockResolvedValue({ status: "connected", identity_switched: false, account });
  mocks.disconnect.mockResolvedValue({ status: "disconnected" });
});

it("requires explicit discard confirmation and preserves connected state on disconnect failure", async () => {
  render(<GoogleAccountSettings identity={identity} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google 로그인 준비" }));
  await waitFor(() => expect(mocks.initialize).toHaveBeenCalledOnce());
  const callback = mocks.initialize.mock.calls[0][0].callback as (response: { credential: string }) => void;
  act(() => callback({ credential: "fixture-proof" }));
  expect(screen.getByRole("dialog", { name: "Google 계정을 연결할까요?" })).toBeTruthy();
  expect(mocks.connect).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(mocks.connect).not.toHaveBeenCalled();
  act(() => callback({ credential: "fixture-proof" }));
  fireEvent.click(screen.getByRole("button", { name: "연결" }));
  await screen.findByText("Google 계정이 연결됐어요.");
  expect(mocks.connect).toHaveBeenCalledExactlyOnceWith(identity, "fixture-proof", "nonce");
  expect(mocks.cancel).toHaveBeenCalled();
  mocks.disconnect.mockRejectedValueOnce(new Error("저장 실패"));
  fireEvent.click(screen.getByRole("button", { name: "연결 해제" }));
  await screen.findByText("저장 실패");
  expect(screen.getByText("Google 계정이 연결됐어요.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "연결 해제" }));
  await screen.findByRole("button", { name: "Google 로그인 준비" });
});

it("shows a failed connection and requires a fresh challenge for a deliberate retry", async () => {
  mocks.connect.mockRejectedValueOnce(new Error("로그인 만료"));
  render(<GoogleAccountSettings identity={identity} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google 로그인 준비" }));
  await waitFor(() => expect(mocks.initialize).toHaveBeenCalledOnce());
  act(() => mocks.initialize.mock.calls[0][0].callback({ credential: "proof" }));
  fireEvent.click(screen.getByRole("button", { name: "연결" }));
  await screen.findByText("로그인 만료");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(mocks.start).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Google 로그인 준비" }));
  await waitFor(() => expect(mocks.start).toHaveBeenCalledTimes(2));
});

it("keeps native central Google login separate and exposes status failures without loading GIS", async () => {
  mocks.desktop.mockReturnValue(true);
  mocks.status.mockRejectedValueOnce(new Error("상태 읽기 실패"));
  render(<GoogleAccountSettings identity={{}} />);
  await screen.findByText("상태 읽기 실패");
  fireEvent.click(screen.getByRole("button", { name: "다시 불러오기" }));
  await screen.findByText("앱의 Google 로그인은 시작 화면의 중앙 계정에서 관리해요.");
  expect(mocks.start).not.toHaveBeenCalled();
  expect(mocks.load).not.toHaveBeenCalled();
});
