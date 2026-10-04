import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { CentralTemporaryError } from "../lib/centralConnectionError";
import { saveCentralDirectoryCache } from "../lib/centralDirectoryCache";
import { useCentralDirectory } from "./useCentralDirectory";
const mocks = vi.hoisted(() => ({ bootstrap: vi.fn(), session: null as object | null }));
vi.mock("../lib/centralIdentity", () => ({ bootstrapCentral: mocks.bootstrap,
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => true, loadCentralSession: () => mocks.session,
  isCentralAuthenticationError: (e: Error) => e.message === "401",
}));
const person = { person_id: "person", display_name: "Name", identity_kind: "google" };
const server = { server_id: "server", alias: "Saved Mac", icon: "", host_os: "macos" as const, relation: "owner" as const };
afterEach(() => { cleanup(); vi.useRealTimers(); vi.resetAllMocks(); localStorage.clear(); });
it("shows cached display only during outage, caps backoff and automatically replaces it on recovery", async () => {
  vi.useFakeTimers();
  mocks.session = { token: "session", person };
  saveCentralDirectoryCache(person.person_id, [server]);
  mocks.bootstrap.mockRejectedValue(new CentralTemporaryError("offline"));
  const { result, unmount } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  expect(result.current.directory).toMatchObject({ status: "central-unconfirmed", servers: [server], live: null });
  for (const delay of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
    const calls = mocks.bootstrap.mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(delay - 1); });
    expect(mocks.bootstrap).toHaveBeenCalledTimes(calls);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(mocks.bootstrap).toHaveBeenCalledTimes(calls + 1);
  }
  mocks.bootstrap.mockResolvedValue({ person, servers: [{ ...server, alias: "Recovered Mac" }], server_time: 1 });
  await act(async () => { window.dispatchEvent(new Event("online")); });
  expect(result.current.directory).toMatchObject({ status: "connected", servers: [{ alias: "Recovered Mac" }] });
  unmount();
  const calls = mocks.bootstrap.mock.calls.length;
  await vi.advanceTimersByTimeAsync(60_000);
  window.dispatchEvent(new Event("online"));
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
  await act(async () => { await vi.advanceTimersByTimeAsync(90_000); window.dispatchEvent(new Event("online")); });
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
