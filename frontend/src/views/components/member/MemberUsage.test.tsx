import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readProviderUsage } from "../../../api/providerUsage";
import { isDesktopWebview } from "../../../lib/desktopBridge";
import type { NativeCliProviderAvailability } from "../../../roomSocketClient";
import MemberUsage from "./MemberUsage";

vi.mock("../../../api/providerUsage", () => ({ readProviderUsage: vi.fn() }));
vi.mock("../../../lib/desktopBridge", () => ({ isDesktopWebview: vi.fn() }));
const provider = { id: "deepseek", usage_supported: true } as NativeCliProviderAvailability;
afterEach(cleanup);
beforeEach(() => { vi.resetAllMocks(); vi.mocked(isDesktopWebview).mockReturnValue(true); });

describe("MemberUsage", () => {
  it("reports that exact provider usage is unsupported", () => {
    render(<MemberUsage displayName="Agent One" />);
    expect(screen.getByRole("region", { name: "Agent One 사용량" })).toBeTruthy();
    expect(screen.getByText(/정확한 잔여량을 제공하지 않습니다/)).toBeTruthy();
    expect(readProviderUsage).not.toHaveBeenCalled();
  });

  it("loads only after an operator action, preserves decimal precision and clears stale data on error", async () => {
    vi.mocked(readProviderUsage).mockResolvedValueOnce({
      provider_id: "deepseek", observed_at: "2026-09-09T00:00:00Z",
      quota: { kind: "balance", is_available: false, balances: [{
        currency: "USD", total_balance: "-0.000000000000000001", granted_balance: "0.00",
        topped_up_balance: "123456789123456789.123456789",
      }] },
    }).mockRejectedValueOnce(new Error("Account authorization rejected"));
    render(<MemberUsage displayName="Agent One" provider={provider} />);
    expect(readProviderUsage).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "계정 사용량 조회" }));
    expect(await screen.findByText(/USD 잔액 -0.000000000000000001/)).toBeTruthy();
    expect(screen.getByText(/123456789123456789.123456789/)).toBeTruthy();
    expect(screen.getByText(/API를 사용할 수 없어요/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "계정 사용량 조회" }));
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Account authorization rejected");
    expect(screen.queryByText(/USD 잔액/)).toBeNull();
  });

  it("keeps a missing native window unknown and distinguishes an unavailable subscription", async () => {
    vi.mocked(readProviderUsage).mockResolvedValueOnce({
      provider_id: "claude", observed_at: "2026-09-09T00:00:00Z",
      quota: { kind: "rate_limits", available: true, windows: [{
        id: "seven_day", label: "7일", used_percent: null, resets_at: null, window_minutes: 10080,
      }] },
    }).mockResolvedValueOnce({
      provider_id: "claude", observed_at: "2026-09-09T00:00:00Z",
      quota: { kind: "rate_limits", available: false, windows: [] },
    });
    render(<MemberUsage displayName="Claude" provider={{ ...provider, id: "claude" }} />);
    fireEvent.click(screen.getByRole("button", { name: "계정 사용량 조회" }));
    expect(await screen.findByText(/7일: 사용량 확인 불가/)).toBeTruthy();
    expect(screen.queryByText(/잔여 100%/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "계정 사용량 조회" }));
    expect(await screen.findByText(/이 계정의 구독 사용량을 확인할 수 없어요/)).toBeTruthy();
  });

  it("does not expose host account usage in the admitted browser", () => {
    vi.mocked(isDesktopWebview).mockReturnValue(false);
    render(<MemberUsage displayName="Agent One" provider={provider} />);
    expect(screen.getByText(/서버 운영자의 데스크톱 앱/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
    expect(readProviderUsage).not.toHaveBeenCalled();
  });
});
