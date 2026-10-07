import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { CentralTemporaryError, fetchCentral } from "../lib/central/connectionError";
import { saveCentralDirectoryCache } from "../lib/central/directoryCache";
import { useCentralDirectory } from "./useCentralDirectory";
const mocks = vi.hoisted(() => ({ bootstrap: vi.fn(), session: null as object | null }));
vi.mock("../lib/central/identity", () => ({ bootstrapCentral: mocks.bootstrap,
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => true, loadCentralSession: () => mocks.session,
  isCentralAuthenticationError: (e: Error) => e.message === "401",
}));
const person = { person_id: "person", display_name: "Name", identity_kind: "google" };
const server = { server_id: "server", alias: "Saved Mac", icon: "", host_os: "macos" as const, relation: "owner" as const };
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.resetAllMocks(); vi.unstubAllGlobals(); localStorage.clear(); });
function wake(trigger: string) {
  if (trigger === "visible") {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
  } else window.dispatchEvent(new Event(trigger));
}
it.each(["focus", "visible", "online", "explicit"])("retains outage display without scheduled retries and recovers on %s", async (trigger) => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  saveCentralDirectoryCache(person.person_id, [server]);
  mocks.bootstrap.mockRejectedValue(new CentralTemporaryError("offline"));
  const { result, unmount } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  expect(result.current.directory).toMatchObject({ status: "central-unconfirmed", servers: [server], live: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(24 * 60 * 60_000); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
  mocks.bootstrap.mockResolvedValue({ person, servers: [{ ...server, alias: "Recovered Mac" }], server_time: 1 });
  await act(async () => { if (trigger === "explicit") await result.current.refresh(); else wake(trigger); });
  expect(result.current.directory).toMatchObject({ status: "connected", servers: [{ alias: "Recovered Mac" }] });
  unmount();
  const calls = mocks.bootstrap.mock.calls.length;
  await vi.advanceTimersByTimeAsync(60_000);
  wake("online"); wake("focus"); wake("visible");
  expect(mocks.bootstrap).toHaveBeenCalledTimes(calls);
});
it("coalesces retry requests, stops at authentication failure and never reads cache as authority", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  saveCentralDirectoryCache(person.person_id, [server]);
  let reject!: (e: Error) => void;
  mocks.bootstrap.mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => {
    const first = result.current.refresh();
    expect(result.current.refresh()).toBe(first);
    reject(new Error("401")); await first;
  });
  expect(result.current.directory).toMatchObject({ status: "authentication-required", person: null, servers: [], live: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(24 * 60 * 60_000); wake("online"); wake("focus"); wake("visible"); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
});
it("does not restore a previous account after a pending outage or unmount", async () => {
  mocks.session = { token: "old", person };
  let reject!: (e: Error) => void;
  mocks.bootstrap.mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
  const { result, unmount } = renderHook(() => useCentralDirectory());
  await act(async () => {
    const pending = result.current.refresh();
    mocks.session = { token: "new", person: { ...person, person_id: "new" } };
    reject(new CentralTemporaryError("offline")); await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });
  expect(result.current.directory).toBeNull();
  const pending = result.current.refresh();
  const signal = mocks.bootstrap.mock.calls.at(-1)![0] as AbortSignal;
  unmount();
  expect(signal.aborted).toBe(true);
  reject(new DOMException("cancelled", "AbortError"));
  await expect(pending).rejects.toThrow("cancelled");
});

it("reports corrupt display cache without blocking existing local-host access", async () => {
  mocks.session = { token: "session", person };
  localStorage.setItem("agentsassemble.centralDirectoryDisplay.v1", "broken-json");
  mocks.bootstrap.mockRejectedValue(new CentralTemporaryError("offline"));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  expect(result.current.directory).toMatchObject({ status: "central-unconfirmed", person, servers: [], live: null });
  expect(result.current.directory?.error).toBeInstanceOf(SyntaxError);
});

it("keeps cached servers visible across startup-to-app handoff while rechecking", () => {
  mocks.session = { token: "session", person };
  saveCentralDirectoryCache(person.person_id, [server]);
  mocks.bootstrap.mockReturnValue(new Promise(() => {}));
  const { result } = renderHook(() => useCentralDirectory(true));
  expect(result.current.directory).toMatchObject({ status: "central-unconfirmed", servers: [server], live: null });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
});

it("rejects a successful flight when the session is cleared before publication", async () => {
  mocks.session = { token: "old", person };
  let complete!: (value: object) => void;
  mocks.bootstrap.mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => {
    const pending = result.current.refresh();
    complete({ person, servers: [server], server_time: 1 });
    mocks.session = null;
    await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  });
  expect(result.current.directory).toBeNull();
});

it("retains saved servers on the first WebKit timeout until an online retry", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  saveCentralDirectoryCache(person.person_id, [server]);
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new DOMException("Fetch is aborted", "AbortError")));
  mocks.bootstrap.mockImplementation(() => fetchCentral("/test", {
    signal: AbortSignal.abort(new DOMException("Timed out", "TimeoutError")),
  }));
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => {});
  expect(result.current.directory).toMatchObject({ status: "central-unconfirmed", person, servers: [server], live: null });
  expect(result.current.directory?.error).toBeUndefined();
  mocks.bootstrap.mockResolvedValue({ person, servers: [server], server_time: 1 });
  await act(async () => { await vi.advanceTimersByTimeAsync(24 * 60 * 60_000); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
  await act(async () => { wake("online"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
  expect(result.current.directory?.status).toBe("connected");
});

it("never polls an idle directory and bypasses focus throttling for explicit and online checks", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  mocks.bootstrap.mockResolvedValue({ person, servers: [server], server_time: 1 });
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(24 * 60 * 60_000); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
  await act(async () => { await result.current.refresh(); });
  await act(async () => { await result.current.refresh(); });
  await act(async () => { wake("online"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(4);
  expect(result.current.directory?.status).toBe("connected");
});

it.each(["focus", "visible"])("throttles %s checks until five minutes after the last successful completion", async (trigger) => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  let complete!: (value: object) => void;
  mocks.bootstrap.mockImplementationOnce(() => new Promise(resolve => { complete = resolve; }));
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  await act(async () => { complete({ person, servers: [server], server_time: 1 }); });
  mocks.bootstrap.mockResolvedValue({ person, servers: [{ ...server, alias: "Changed" }], server_time: 1 });
  await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000 - 1); wake(trigger); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
  await act(async () => { await vi.advanceTimersByTimeAsync(1); wake(trigger); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
  expect(result.current.directory?.servers[0].alias).toBe("Changed");
  await act(async () => { wake("focus"); wake("visible"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
});

it("ignores hidden visibility and coalesces focus, visible, online and explicit checks", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  mocks.bootstrap.mockResolvedValueOnce({ person, servers: [server], server_time: 1 });
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000); });
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
  expect(mocks.bootstrap).toHaveBeenCalledOnce();
  let complete!: (value: object) => void;
  mocks.bootstrap.mockImplementationOnce(() => new Promise(resolve => { complete = resolve; }));
  await act(async () => { wake("focus"); wake("visible"); wake("online"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
  await act(async () => {
    const pending = result.current.refresh();
    complete({ person, servers: [server], server_time: 1 });
    await pending;
  });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
});

it("does not throttle a new account using the old account's successful check", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "old", person };
  mocks.bootstrap.mockResolvedValueOnce({ person, servers: [server], server_time: 1 });
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => {});
  const nextPerson = { ...person, person_id: "new" };
  mocks.session = { token: "new", person: nextPerson };
  mocks.bootstrap.mockResolvedValueOnce({ person: nextPerson, servers: [], server_time: 1 });
  await act(async () => { wake("focus"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(2);
  expect(result.current.directory).toMatchObject({ person: nextPerson, servers: [] });
});

it("waits for the next eligible focus after repeated failures, including a non-temporary error", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  mocks.bootstrap.mockResolvedValueOnce({ person, servers: [server], server_time: 1 });
  const { result } = renderHook(() => useCentralDirectory(true));
  await act(async () => {});
  mocks.bootstrap.mockRejectedValueOnce(new CentralTemporaryError("temporary_capacity_exhausted"));
  await act(async () => { await vi.advanceTimersByTimeAsync(5 * 60_000); wake("focus"); });
  expect(result.current.directory?.status).toBe("central-unconfirmed");
  mocks.bootstrap.mockRejectedValueOnce(new Error("invalid directory"));
  await act(async () => { wake("visible"); });
  expect(result.current.directory?.status).toBe("error");
  mocks.bootstrap.mockResolvedValueOnce({ person, servers: [server], server_time: 1 });
  await act(async () => { await vi.advanceTimersByTimeAsync(24 * 60 * 60_000); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(3);
  await act(async () => { wake("focus"); });
  expect(mocks.bootstrap).toHaveBeenCalledTimes(4);
  expect(result.current.directory?.status).toBe("connected");
});
