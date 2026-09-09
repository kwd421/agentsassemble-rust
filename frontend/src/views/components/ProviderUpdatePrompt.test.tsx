import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { providerUpdateOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import ProviderUpdatePrompt from "./ProviderUpdatePrompt";

vi.mock("../../api/providerOperations", () => ({ providerUpdateOperation: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ openProviderSetupHelp: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const offer = { provider_id: "grok", current_version: "1.0.5", latest_version: "1.0.24",
  update_available: true, native_update: true, observed_at: "2026-09-09T00:00:00Z", handoff_started: false };

it("only checks on demand, Later has no update effect, and explicit consent carries the displayed offer", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValue(offer);
  render(<ProviderUpdatePrompt providerId="grok" />);
  expect(providerUpdateOperation).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "새 버전 확인" }));
  await screen.findByText("새 버전이 있어요. 지금 업데이트할까요?");
  fireEvent.click(screen.getByRole("button", { name: "나중에" }));
  expect(providerUpdateOperation).toHaveBeenCalledExactlyOnceWith("grok", undefined);
  expect(screen.queryByRole("button", { name: "업데이트" })).toBeNull();
  expect(screen.getByText(/현재 버전으로 계속/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "새 버전 확인" }));
  await screen.findByRole("button", { name: "업데이트" });
  vi.mocked(providerUpdateOperation).mockResolvedValue({ ...offer, handoff_started: true });
  fireEvent.click(screen.getByRole("button", { name: "업데이트" }));
  await screen.findByText(/업데이트 터미널을 열었어요/);
  expect(providerUpdateOperation).toHaveBeenLastCalledWith("grok", "1.0.24");
  expect(openProviderSetupHelp).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "업데이트" })).toBeNull();
});

it("keeps a lost update response uncertain instead of claiming no update started", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValue(offer);
  render(<ProviderUpdatePrompt providerId="grok" />);
  fireEvent.click(screen.getByRole("button", { name: "새 버전 확인" }));
  await screen.findByRole("button", { name: "업데이트" });
  vi.mocked(providerUpdateOperation).mockRejectedValue(new TypeError("Failed to fetch"));
  fireEvent.click(screen.getByRole("button", { name: "업데이트" }));
  expect(await screen.findByText(/업데이트 터미널이 열렸을 수/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "업데이트" })).toBeNull();
});
