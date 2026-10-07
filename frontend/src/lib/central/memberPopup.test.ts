// @vitest-environment-options {"url":"https://central.test/"}
import { afterEach, expect, it, vi } from "vitest";
vi.mock("./identity", () => ({ isCentralWebEntry: () => true, loadCentralSession: () => ({ token: "central-session" }) }));
import { loginMemberPopup } from "./memberPopup";
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });
it("accepts completion only from its exact popup with matching origin, nonce and OAuth state", async () => {
  vi.useFakeTimers();
  const popup = { close: vi.fn(), closed: false } as unknown as Window;
  const open = vi.spyOn(window, "open").mockReturnValue(popup);
  const result = loginMemberPopup(new AbortController().signal);
  const finished = vi.fn(); void result.then(finished);
  const nonce = new URL(String(open.mock.calls[0][0]), window.location.href).searchParams.get("member-login");
  const state = "A".repeat(43);
  const send = (type: string, overrides = {}, origin = window.location.origin, source = popup) => window.dispatchEvent(new MessageEvent("message", { origin, source, data: { type, nonce, state, ...overrides } }));
  send("member-login-started", { expiresAt: Math.floor(Date.now() / 1000) + 300 });
  send("member-login-complete", {}, "https://host.test");
  send("member-login-complete", {}, window.location.origin, window);
  send("member-login-complete", { state: "B".repeat(43) });
  await Promise.resolve(); expect(finished).not.toHaveBeenCalled();
  send("member-login-complete"); await result;
  expect(popup.close).toHaveBeenCalledOnce();
  expect(String(open.mock.calls[0][0])).not.toContain("invite");
});
it("ends a blocked or expired popup without navigation or polling", async () => {
  vi.useFakeTimers();
  vi.spyOn(window, "open").mockReturnValue(null);
  await expect(loginMemberPopup(new AbortController().signal)).rejects.toThrow("원래 초대 링크");
  const popup = { close: vi.fn(), closed: false } as unknown as Window;
  vi.mocked(window.open).mockReturnValue(popup);
  const result = loginMemberPopup(new AbortController().signal);
  const rejection = expect(result).rejects.toThrow("원래 초대 링크");
  await vi.advanceTimersByTimeAsync(300_000); await rejection;
  expect(popup.close).toHaveBeenCalledOnce();
});
