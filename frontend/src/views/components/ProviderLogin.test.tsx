import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { postJsonServerOperator } from "../../api/http";
import ProviderLogin from "./ProviderLogin";
import { ApiError } from "../../lib/apiErrors";

vi.mock("../../api/http", () => ({ postJsonServerOperator: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true }));
afterEach(cleanup);
beforeEach(() => vi.resetAllMocks());

it.each([
  ["started", "터미널에서 로그인을 마친 뒤 카탈로그를 갱신해 주세요."],
  ["authenticated", "로그인을 완료했어요."],
])("renders the server's %s receipt without inferring authentication", async (status, message) => {
  vi.mocked(postJsonServerOperator).mockResolvedValue({ provider_id: "opencode", status });
  render(<ProviderLogin providerId="opencode" displayName="OpenCode" />);
  fireEvent.click(screen.getByRole("button", { name: "OpenCode 로그인" }));
  expect(await screen.findByText(message)).toBeTruthy();
  expect(postJsonServerOperator).toHaveBeenCalledExactlyOnceWith("/api/providers/login", { provider_id: "opencode" });
  if (status === "started") expect(screen.queryByText("로그인을 완료했어요.")).toBeNull();
});

it.each([true, false])("keeps confirmed cancellation clear when cancel reply arrives first: %s", async (cancelFirst) => {
  let rejectLogin!: (error: Error) => void;
  let resolveCancel!: (value: unknown) => void;
  vi.mocked(postJsonServerOperator).mockImplementation((path) => path.endsWith("/cancel")
    ? new Promise((resolve) => { resolveCancel = resolve; })
    : new Promise((_, reject) => { rejectLogin = reject; }));
  render(<ProviderLogin providerId="codex" displayName="Codex" />);
  fireEvent.click(screen.getByRole("button", { name: "Codex 로그인" }));
  fireEvent.click(screen.getByRole("button", { name: "로그인 취소" }));
  const cancelled = () => rejectLogin(new ApiError(409, "Provider login was cancelled.", "provider_login_cancelled"));
  const receipt = () => resolveCancel({ provider_id: "codex", status: "cancelled" });
  await act(async () => { (cancelFirst ? receipt : cancelled)(); });
  await act(async () => { (cancelFirst ? cancelled : receipt)(); });
  expect(screen.getByText("로그인을 취소했어요.")).toBeTruthy();
  expect(screen.queryByText("Provider login was cancelled.")).toBeNull();
});
