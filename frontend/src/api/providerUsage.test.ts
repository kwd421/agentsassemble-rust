import { beforeEach, describe, expect, it, vi } from "vitest";
import { postJsonServerOperator } from "./http";
import { readProviderUsage } from "./providerUsage";
vi.mock("./http", () => ({ postJsonServerOperator: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

describe("provider usage response boundary", () => {
  it("retains nullable native measurements and rejects account substitution or malformed percentages", async () => {
    const response = { provider_id: "claude", observed_at: "2026-09-09T00:00:00Z", quota: {
      kind: "rate_limits", available: true, windows: [{ id: "five_hour", label: "5시간",
        used_percent: null, resets_at: null, window_minutes: 300 }],
    } };
    vi.mocked(postJsonServerOperator).mockResolvedValueOnce(response);
    expect(await readProviderUsage("claude")).toEqual(response);
    for (const invalid of [
      { ...response, provider_id: "codex" },
      { ...response, quota: { ...response.quota, windows: [{ ...response.quota.windows[0], used_percent: "0" }] } },
      { ...response, quota: { ...response.quota, available: false } },
    ]) {
      vi.mocked(postJsonServerOperator).mockResolvedValueOnce(invalid);
      await expect(readProviderUsage("claude")).rejects.toThrow("사용량 응답");
    }
  });
});
