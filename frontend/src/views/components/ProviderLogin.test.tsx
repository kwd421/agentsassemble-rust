import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { postJsonServerOperator } from "../../api/http";
import ProviderLogin from "./ProviderLogin";

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
