import { afterEach, expect, it, vi } from "vitest";
import { loadCentralSession, loginCentralGoogle } from "./centralIdentity";

const { control } = vi.hoisted(() => ({ control: vi.fn() }));
vi.mock("./desktopBridge", () => ({ controlDesktopCentralLogin: control, openDesktopCentralGoogleLogin: vi.fn() }));
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); control.mockReset(); localStorage.clear(); });

it("retires the native return if cancellation arrives while start is in flight", async () => {
  const controller = new AbortController();
  control.mockImplementation(async (action: string) => {
    if (action === "start") {
      controller.abort();
      return { redirect_uri: "http://127.0.0.1:43210/api/central-login/callback", result: { status: "pending", expires_at: 9_999_999_999 } };
    }
    return { result: { status: "cancelled" } };
  });
  await expect(loginCentralGoogle(undefined, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
  expect(control.mock.calls.map(([action]) => action)).toEqual(["start", "cancel"]);
  expect(control.mock.calls[1][1]).toBe(control.mock.calls[0][1]);
});

it("attempts retirement after an uncertain native start and exposes the failure", async () => {
  control.mockRejectedValueOnce(new Error("native start failed")).mockResolvedValueOnce({ result: { status: "cancelled" } });
  await expect(loginCentralGoogle()).rejects.toThrow("native start failed");
  expect(control.mock.calls.map(([action]) => action)).toEqual(["start", "cancel"]);
});

function completedHandoff(controller: AbortController) {
  const events: string[] = [];
  // Return an existing device from the browser store; no device generation or
  // network identity proof is involved in these completion-order cases.
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: {
      objectStoreNames: { contains: () => true }, close: vi.fn(),
      transaction: () => ({ objectStore: () => ({ get: () => {
        const read = { result: { deviceId: "fixture-device", privateKey: {}, publicJwk: {} }, onsuccess: () => {} };
        queueMicrotask(() => read.onsuccess());
        return read;
      } }) }),
    }, onsuccess: () => {} };
    queueMicrotask(() => request.onsuccess());
    return request;
  } });
  vi.spyOn(window, "setTimeout").mockImplementation((callback: TimerHandler) => {
    queueMicrotask(() => { if (typeof callback === "function") callback(); });
    return 1;
  });
  control.mockImplementation(async (action: string) => {
    events.push(action);
    if (action === "start") return { redirect_uri: "http://127.0.0.1:43210/api/central-login/callback", result: { status: "pending", expires_at: 9_999_999_999 } };
    if (action === "poll") return { result: { status: "complete", authorization_code: "fixture-authorization-code" } };
    return { result: { status: "cancelled" } };
  });
  const fetcher = vi.fn(async (input: string, init: RequestInit) => {
    if (input.endsWith("/start")) {
      const body = JSON.parse(String(init.body));
      const query = new URLSearchParams({ ...body, client_id: "fixture-client", response_type: "code", scope: "openid", nonce: "fixture-nonce", code_challenge_method: "S256" });
      return Response.json({ handoff_id: "fixture-handoff", authorization_url: `https://accounts.google.com/o/oauth2/v2/auth?${query}`, state: body.state, expires_at: 9_999_999_999 });
    }
    events.push("exchange");
    return { ok: true, json: async () => {
      controller.abort();
      return { status: "complete", person: { person_id: "fixture-person", display_name: "Google user", identity_kind: "google" }, session: { token: "fixture-session", expires_at: 9_999_999_999, device_id: "fixture-device" } };
    } } as Response;
  });
  vi.stubGlobal("fetch", fetcher);
  return { events, fetcher };
}

it("preserves a completed exchange despite a simultaneous cancellation", async () => {
  const controller = new AbortController();
  const { events } = completedHandoff(controller);
  await expect(loginCentralGoogle(undefined, controller.signal)).resolves.toMatchObject({ token: "fixture-session" });
  expect(loadCentralSession()?.token).toBe("fixture-session");
  expect(events).toEqual(["start", "poll", "cancel", "exchange"]);
});

it("exposes retirement failure before exchanging any authorization code", async () => {
  const controller = new AbortController();
  const { events } = completedHandoff(controller);
  const previous = control.getMockImplementation()!;
  control.mockImplementation(async (action: string, state: string) => {
    if (action === "cancel") throw new Error("retirement failed");
    return previous(action, state);
  });
  await expect(loginCentralGoogle(undefined, controller.signal)).rejects.toThrow("retirement failed");
  expect(events).not.toContain("exchange");
  expect(loadCentralSession()).toBeNull();
});
