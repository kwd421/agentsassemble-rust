import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bootstrapCentral, clearCentralSession, fetchCentralServerIcon, loadCentralSession,
  registerLocalServer, renameCentralServer, saveSession, setCentralServerIcon, unsignedPost } from "../lib/centralIdentity";
import { loadCentralDirectoryCache, saveCentralDirectoryCache } from "../lib/centralDirectoryCache";
import { useCentralDirectory } from "./useCentralDirectory";

const legacyKey = "agentsassemble.centralServers.v1";
const person = { person_id: "account", display_name: "Name", identity_kind: "google" as const };
const session = { token: "fixture-token", device_id: "fixture-device", expires_at: 9_999_999_999 };
const server = { server_id: "server", alias: "My Mac", icon: "/v1/servers/server/icon/version.png",
  host_os: "macos" as const, relation: "owner" as const };
const directory = { person, servers: [server], server_time: 1 };
const fetcher = vi.fn();
function seedLegacy() {
  localStorage.setItem(legacyKey, JSON.stringify([{ ...server,
    endpoint: { origin: "https://old-host.example" }, host_public_key_jwk: { x: "old-host-key" } }]));
}
beforeEach(async () => {
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
  const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
  const device = { deviceId: session.device_id, privateKey: pair.privateKey, publicJwk: {} };
  vi.stubGlobal("indexedDB", { open: () => {
    const request = { result: { close() {}, transaction: () => ({ objectStore: () => ({ get: () => {
      const read = { result: device, onsuccess() {} };
      queueMicrotask(() => read.onsuccess()); return read;
    } }) }) }, onsuccess() {} };
    queueMicrotask(() => request.onsuccess()); return request;
  } });
  vi.stubGlobal("fetch", fetcher);
  saveSession({ person, session });
  saveCentralDirectoryCache(person.person_id, [server]);
});
afterEach(() => {
  cleanup(); clearCentralSession(); localStorage.clear(); fetcher.mockReset();
  vi.useRealTimers(); vi.unstubAllGlobals(); vi.unstubAllEnvs();
});

it.each(["bootstrap", "unsigned login"])("deletes the legacy directory before offline %s network work", async (entry) => {
  seedLegacy();
  const seen: Array<string | null> = [];
  fetcher.mockImplementation(async () => {
    seen.push(localStorage.getItem(legacyKey));
    throw new TypeError("offline");
  });
  const request = entry === "bootstrap" ? bootstrapCentral() : unsignedPost("/v1/auth/guest", {});
  await expect(request).rejects.toThrow();
  expect(seen).toEqual([null]);
  expect(localStorage.getItem(legacyKey)).toBeNull();
  expect(loadCentralSession()?.token).toBe(session.token);
});

const operations = [
  ["bootstrap", () => bootstrapCentral()],
  ["icon read", () => fetchCentralServerIcon(server.icon)],
  ["rename", () => renameCentralServer(server, "Changed")],
  ["registration", () => registerLocalServer("fixture-local-token")],
  ["icon write", () => setCentralServerIcon(server, null)],
] as const;
it.each(operations)("immediately requires authentication on signed %s 401 and stops polling", async (_name, operation) => {
  fetcher.mockResolvedValueOnce(Response.json(directory));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  expect(result.current.directory?.status).toBe("connected");
  seedLegacy();
  vi.useFakeTimers();
  fetcher.mockImplementation(async (url: string) => url === "/api/central-directory/registration-proof"
    ? Response.json({ server_id: server.server_id, host_name: "My Mac", host_os: "macos",
      host_public_key_jwk: {}, host_registration_proof: {} })
    : Response.json({ error: { code: "invalid_session" } }, { status: 401 }));
  await act(async () => { await expect(operation()).rejects.toThrow(); });
  expect(loadCentralSession()).toBeNull();
  expect(localStorage.getItem(legacyKey)).toBeNull();
  expect(loadCentralDirectoryCache(person.person_id)).toEqual([]);
  expect(result.current.directory).toMatchObject({ status: "authentication-required", person: null, servers: [], live: null });
  const calls = fetcher.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(90_000); window.dispatchEvent(new Event("online")); });
  expect(fetcher).toHaveBeenCalledTimes(calls);
});

it("preserves the new account when an old signed request returns 401", async () => {
  fetcher.mockResolvedValueOnce(Response.json(directory));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  let respond!: (response: Response) => void;
  let started!: () => void;
  const ready = new Promise<void>((resolve) => { started = resolve; });
  fetcher.mockImplementationOnce(() => new Promise<Response>((resolve) => { respond = resolve; started(); }));
  const oldRequest = fetchCentralServerIcon(server.icon);
  await ready;
  const nextPerson = { ...person, person_id: "new-account" };
  saveSession({ person: nextPerson, session: { ...session, token: "new-token" } });
  fetcher.mockResolvedValueOnce(Response.json({ ...directory, person: nextPerson }));
  await act(async () => { await result.current.refresh(); });
  await act(async () => {
    respond(Response.json({}, { status: 401 }));
    await expect(oldRequest).rejects.toThrow();
  });
  expect(loadCentralSession()?.token).toBe("new-token");
  expect(loadCentralDirectoryCache(nextPerson.person_id)).toEqual([server]);
  expect(result.current.directory).toMatchObject({ status: "connected", person: nextPerson });
});

it("clears the directory as soon as 401 headers arrive, before the error body completes", async () => {
  fetcher.mockResolvedValueOnce(Response.json(directory));
  const { result } = renderHook(() => useCentralDirectory());
  await act(async () => { await result.current.refresh(); });
  let finishBody!: (body: object) => void;
  let readingBody!: () => void;
  const reading = new Promise<void>((resolve) => { readingBody = resolve; });
  const response = Response.json({}, { status: 401 });
  vi.spyOn(response, "json").mockImplementation(() => new Promise((resolve) => {
    finishBody = resolve; readingBody();
  }));
  fetcher.mockResolvedValueOnce(response);
  const request = fetchCentralServerIcon(server.icon);
  await act(async () => { await reading; });
  expect(loadCentralSession()).toBeNull();
  expect(result.current.directory).toMatchObject({ status: "authentication-required", servers: [], person: null, live: null });
  finishBody({ error: { code: "invalid_session" } });
  await expect(request).rejects.toThrow();
});
