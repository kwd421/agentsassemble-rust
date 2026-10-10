// @vitest-environment-options {"url":"https://central.test/"}
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ complete: vi.fn(), start: vi.fn(), clear: vi.fn(), session: vi.fn() }));
vi.mock("./identity", () => ({ isCentralWebEntry: () => true, loadCentralSession: mocks.session }));
vi.mock("./webGoogle", () => ({ completeCentralWebGoogleReturn: mocks.complete, startCentralWebGoogle: mocks.start, clearCentralWebGooglePending: mocks.clear }));
import { loginMemberPopup, runMemberLoginPopup, registerMemberLoginPopup } from "./memberPopup";
import { GoogleRegistrationRequired } from "./googleRegistration";
const KEY = "agentsassemble.memberLoginPopup.v1";
class Channel {
  static channels: Channel[] = [];
  onmessage: ((event: { data: unknown }) => void) | null = null;
  closed = false;
  constructor(readonly name: string) { Channel.channels.push(this); }
  postMessage(data: unknown) {
    for (const peer of Channel.channels) if (peer !== this && peer.name === this.name && !peer.closed) {
      queueMicrotask(() => { if (!peer.closed) peer.onmessage?.({ data }); });
    }
  }
  close() { this.closed = true; }
}
beforeEach(() => {
  vi.useFakeTimers(); vi.stubGlobal("BroadcastChannel", Channel); Channel.channels = [];
  sessionStorage.clear(); window.history.replaceState({}, "", "/");
  mocks.session.mockReturnValue({ token: "central-session" }); mocks.complete.mockResolvedValue(undefined);
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); vi.useRealTimers(); });
function begin() {
  const popup = { close: vi.fn(), closed: false } as unknown as Window;
  const open = vi.spyOn(window, "open").mockReturnValue(popup);
  const controller = new AbortController(), result = loginMemberPopup(controller.signal);
  const nonce = new URL(String(open.mock.calls[0][0]), window.location.href).searchParams.get("member-login")!;
  const state = "A".repeat(43), expiresAt = Math.floor(Date.now() / 1000) + 300;
  const channel = new Channel(`${KEY}:${nonce}`);
  const send = (overrides = {}, origin = window.location.origin, source = popup) => window.dispatchEvent(new MessageEvent("message", { origin, source, data: { type: "member-login-started", nonce, state, expiresAt, ...overrides } }));
  return { popup, open, controller, result, nonce, state, expiresAt, channel, send };
}
it("correlates the exact popup first; nonce channel return survives a COOP-severed opener", async () => {
  const b = begin(), finished = vi.fn(); void b.result.then(finished);
  b.send({}, "https://host.test"); b.send({}, window.location.origin, window); b.send({ nonce: "B".repeat(43) });
  b.channel.postMessage({ type: "member-login-returned", nonce: b.nonce, state: b.state });
  b.channel.postMessage({ type: "member-login-complete", nonce: b.nonce, state: b.state });
  await Promise.resolve(); expect(finished).not.toHaveBeenCalled();
  b.send();
  b.channel.postMessage({ type: "member-login-complete", nonce: b.nonce, state: b.state });
  await Promise.resolve(); expect(finished).not.toHaveBeenCalled();
  Object.defineProperty(b.popup, "closed", { value: true }); window.dispatchEvent(new Event("focus"));
  vi.stubGlobal("opener", null);
  sessionStorage.setItem(KEY, JSON.stringify({ nonce: b.nonce, state: b.state, expiresAt: b.expiresAt }));
  window.history.replaceState({}, "", `/?code=fixture-code&state=${b.state}`);
  mocks.complete.mockImplementationOnce(async () => window.history.replaceState({}, "", "/"));
  await runMemberLoginPopup(new AbortController().signal); await b.result;
  expect(mocks.complete).toHaveBeenCalledOnce(); expect(b.popup.close).toHaveBeenCalledOnce();
  expect(window.location.search).toBe(""); expect(sessionStorage.getItem(KEY)).toBeNull();
  expect(Channel.channels[0].closed).toBe(true);
  expect(String(b.open.mock.calls[0][0])).not.toContain("invite");
});
it("shows an actionable blocked error, bounds expiry and disposes cancellation without polling", async () => {
  vi.spyOn(window, "open").mockReturnValue(null);
  await expect(loginMemberPopup(new AbortController().signal)).rejects.toThrow("팝업을 허용한 뒤 Google로 다시 시도");
  expect(Channel.channels[0].closed).toBe(true); vi.restoreAllMocks();
  const b = begin(); b.send();
  const rejected = expect(b.result).rejects.toThrow("초대 창에서 다시 시도");
  await vi.advanceTimersByTimeAsync(300_000); await rejected;
  expect(Channel.channels[1].closed).toBe(true);
  vi.restoreAllMocks(); const cancelled = begin(); cancelled.send();
  const aborted = expect(cancelled.result).rejects.toThrow("초대 창에서 다시 시도");
  cancelled.controller.abort(); await aborted; expect(cancelled.popup.close).toHaveBeenCalledOnce();
});
it("rejects callback mismatch and missing live opener wait; clears callback state without exchanging", async () => {
  const pending = { nonce: "N".repeat(43), state: "A".repeat(43), expiresAt: Date.now() / 1000 + 300 };
  sessionStorage.setItem(KEY, JSON.stringify(pending));
  window.history.replaceState({}, "", `/?code=fixture-code&state=${"B".repeat(43)}`);
  await expect(runMemberLoginPopup(new AbortController().signal)).rejects.toThrow();
  expect(window.location.search).toBe(""); expect(sessionStorage.getItem(KEY)).toBeNull();
  sessionStorage.setItem(KEY, JSON.stringify(pending));
  window.history.replaceState({}, "", `/?code=fixture-code&state=${pending.state}`);
  const rejected = expect(runMemberLoginPopup(new AbortController().signal)).rejects.toThrow();
  await vi.advanceTimersByTimeAsync(10_000); await rejected;
  expect(mocks.complete).not.toHaveBeenCalled(); expect(mocks.clear).toHaveBeenCalled();
});
it.each(["absent", "deleted"] as const)("retains %s only for explicit registration and correlated completion", async identityStatus => {
  const b = begin(); b.send(); vi.stubGlobal("opener", null);
  sessionStorage.setItem(KEY, JSON.stringify({ nonce: b.nonce, state: b.state, expiresAt: b.expiresAt }));
  window.history.replaceState({}, "", `/?code=fixture-code&state=${b.state}`);
  const registration = new GoogleRegistrationRequired(identityStatus, "web", { handoff_id: "goh_fixture", code_verifier: "V".repeat(43), authorization_code: "fixture-code" }, b.expiresAt, null);
  const register = vi.spyOn(registration, "register").mockResolvedValue({} as never);
  mocks.complete.mockRejectedValueOnce(registration);
  await expect(runMemberLoginPopup(new AbortController().signal)).rejects.toBe(registration);
  expect(register).not.toHaveBeenCalled(); expect(sessionStorage.getItem(KEY)).not.toBeNull(); expect(window.location.search).toBe("");
  await registerMemberLoginPopup(registration, new AbortController().signal); await b.result;
  expect(register).toHaveBeenCalledOnce(); expect(sessionStorage.getItem(KEY)).toBeNull();
});
