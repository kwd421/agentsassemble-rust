import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { authorizeDeletionGooglePopup, returnDeletionGooglePopup } from "./deletionGooglePopup";
vi.mock("./identity", () => ({ isCentralWebEntry: () => true }));
class Channel {
  static channels: Channel[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  closed = false;
  constructor(readonly name: string) { Channel.channels.push(this); }
  postMessage(data: unknown) { for (const peer of Channel.channels) if (peer !== this && peer.name === this.name && !peer.closed) queueMicrotask(() => peer.onmessage?.({ data } as MessageEvent)); }
  close() { this.closed = true; }
}
const KEY = "agentsassemble.deletionGooglePopup.v1";
beforeEach(() => { Channel.channels = []; vi.stubGlobal("BroadcastChannel", Channel); sessionStorage.clear(); window.history.replaceState({}, "", "/?room=original"); });
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });
it("keeps the origin room while a COOP-severed callback returns only the correlated code", async () => {
  const store = new Map<string, string>();
  let navigate!: (url: string) => void;
  const navigated = new Promise<string>(resolve => { navigate = resolve; });
  const popup = { close: vi.fn(), sessionStorage: { setItem: (k: string, v: string) => store.set(k, v), getItem: (k: string) => store.get(k) }, location: { replace: navigate } };
  vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  const result = authorizeDeletionGooglePopup(async body => ({ handoff_id: "fixture", state: body.state, expires_at: Math.floor(Date.now()/1000)+300,
    authorization_url: `https://accounts.google.com/o/oauth2/v2/auth?redirect_uri=${encodeURIComponent(window.location.origin + "/")}&state=${body.state}&code_challenge=${body.code_challenge}` }), new AbortController().signal);
  const url = new URL(await navigated), state = url.searchParams.get("state")!;
  const channel = new Channel(`${KEY}:${state}`);
  channel.postMessage({ type: "deletion-google-return", state: "wrong", code: "wrong-code" });
  channel.postMessage({ type: "deletion-google-return", state, code: "fixture-code" });
  expect(await result).toMatchObject({ authorization_code: "fixture-code" });
  expect(window.location.search).toBe("?room=original");
  expect(popup.close).toHaveBeenCalledOnce();
  expect([...store.values()].join()).not.toContain("verifier");
});
it("cancellation during start closes the owned popup without navigation", async () => {
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  const popup = { close: vi.fn(), location: { replace: vi.fn() } };
  vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  const abort = new AbortController();
  const result = authorizeDeletionGooglePopup(async body => { await pending; return { handoff_id: "fixture", state: body.state, expires_at: 2 ** 31, authorization_url: "https://accounts.google.com/o/oauth2/v2/auth" }; }, abort.signal);
  abort.abort(); release();
  await expect(result).rejects.toThrow();
  expect(popup.location.replace).not.toHaveBeenCalled(); expect(popup.close).toHaveBeenCalledOnce();
});
it("callback sanitizes the code URL and cannot complete a wrong-state return", async () => {
  const state = "s".repeat(43);
  sessionStorage.setItem(KEY, JSON.stringify({ state, expiresAt: Math.floor(Date.now()/1000)+300 }));
  window.history.replaceState({}, "", "/?code=fixture-code&state=wrong");
  await expect(returnDeletionGooglePopup(new AbortController().signal)).rejects.toThrow();
  expect(window.location.search).toBe(""); expect(sessionStorage.getItem(KEY)).toBeNull();
});

it("bounds a stalled start request from popup creation and closes it on expiry", async () => {
  const expiry = new AbortController();
  vi.spyOn(AbortSignal, "timeout").mockReturnValue(expiry.signal);
  const popup = { close: vi.fn(), location: { replace: vi.fn() } };
  vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  let started!: () => void;
  const ready = new Promise<void>(resolve => { started = resolve; });
  let startSignal: AbortSignal | undefined;
  const result = authorizeDeletionGooglePopup(async (_body, signal) => {
    startSignal = signal; started(); return new Promise(() => {});
  }, new AbortController().signal);
  await ready;
  expiry.abort(new DOMException("expired", "TimeoutError"));
  expect(startSignal?.aborted).toBe(true);
  await expect(result).rejects.toThrow();
  expect(AbortSignal.timeout).toHaveBeenCalledWith(600_000);
  expect(popup.close).toHaveBeenCalledOnce(); expect(popup.location.replace).not.toHaveBeenCalled();
});
