import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bootstrapCentral, clearCentralSession, fetchCentralServerIcon, loadCentralSession,
  registerLocalServer, openCentralOwnedServer, renameCentralServer, saveSession, setCentralServerIcon, unsignedPost } from "../lib/central/identity";
import { loadCentralDirectoryCache, saveCentralDirectoryCache } from "../lib/central/directoryCache";
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

it.each([undefined, "stored-epoch"])("sends stored epoch on device mutations (%s)", async (epoch) => {
  const current = { ...server, registration_epoch: epoch };
  fetcher.mockImplementation(async () => Response.json({ icon: "" }));
  await renameCentralServer(current, "New name");
  await setCentralServerIcon(current, null);
  await expect(openCentralOwnedServer({ ...current, host_public_key_jwk: {}, host_key_fingerprint: "key",
    endpoint: { status: "likely_online", origin: "https://host.example", generation: 1, lease_expires_at: 9_999_999_999 } })).rejects.toThrow();
  for (const [, init] of fetcher.mock.calls) {
    const body = JSON.parse(init.body);
    expect(body.registration_epoch).toBe(epoch);
    expect(Object.hasOwn(body, "registration_epoch")).toBe(epoch !== undefined);
    expect(init.headers["x-aa-signature"]).toBeTruthy();
  }
});

it("retains the bootstrap epoch with the server through cache reload", async () => {
  fetcher.mockResolvedValue(Response.json({ ...directory, servers: [{ ...server, registration_epoch: "bootstrap-epoch" }] }));
  await bootstrapCentral();
  expect(loadCentralDirectoryCache(person.person_id)[0].registration_epoch).toBe("bootstrap-epoch");
});

function registrationFixture(initialEpoch: string | undefined, failures = 0, code = "incarnation_conflict") {
  let epoch = initialEpoch;
  const sent: Array<Record<string, unknown>> = [];
  const updates: Array<string | null> = [];
  fetcher.mockImplementation(async (url: string, init: RequestInit) => {
    const body = JSON.parse(String(init.body));
    if (url === "/api/central-directory/registration-proof") {
      if (Object.hasOwn(body, "server_id")) {
        updates.push(body.registration_epoch);
        epoch = body.registration_epoch ?? undefined;
        return Response.json({ status: "ok" });
      }
      return Response.json({ server_id: server.server_id, host_name: "My Mac", host_os: "macos",
        host_public_key_jwk: {}, host_registration_proof: {}, registration_epoch: epoch });
    }
    sent.push(body);
    if (sent.length <= failures) return Response.json({ error: { code } }, { status: 409 });
    return Response.json({ registration_epoch: "returned-epoch" });
  });
  return { sent, updates };
}

it.each([undefined, "existing-epoch"])("persists registration response and fills an epoch-less upgrade (%s)", async (epoch) => {
  const fixture = registrationFixture(epoch);
  await registerLocalServer("local-device");
  expect(fixture.sent).toHaveLength(1);
  expect(fixture.sent[0].registration_epoch).toBe(epoch);
  expect(Object.hasOwn(fixture.sent[0], "registration_epoch")).toBe(epoch !== undefined);
  expect(fixture.updates).toEqual(["returned-epoch"]);
});

it.each([1, 2])("clears stale epoch and re-registers at most once (%s conflicts)", async (failures) => {
  const fixture = registrationFixture("old-epoch", failures);
  const request = registerLocalServer("local-device");
  if (failures === 1) await expect(request).resolves.toBeUndefined();
  else await expect(request).rejects.toMatchObject({ status: 409, code: "incarnation_conflict" });
  expect(fixture.sent).toHaveLength(2);
  expect(fixture.sent[0].registration_epoch).toBe("old-epoch");
  expect(fixture.sent[1]).not.toHaveProperty("registration_epoch");
  expect(fixture.sent[0].host_public_key_jwk).toEqual(fixture.sent[1].host_public_key_jwk);
  expect(fixture.updates).toEqual(failures === 1 ? [null, "returned-epoch"] : [null]);
});

it("does not clear epoch or retry an ownership claim or another registration error", async () => {
  const fixture = registrationFixture("old-epoch", 2);
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ ...loadCentralSession(), pending_account_switch: true }));
  await expect(registerLocalServer("local-device")).rejects.toMatchObject({ code: "incarnation_conflict" });
  expect(fixture.sent).toHaveLength(1);
  expect(fixture.sent[0]).toMatchObject({ registration_epoch: "old-epoch", claim_ownership: true });
  expect(fixture.updates).toEqual([]);
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({ ...session, person }));
  const denied = registrationFixture("old-epoch", 2, "server_key_conflict");
  await expect(registerLocalServer("local-device")).rejects.toMatchObject({ code: "server_key_conflict" });
  expect(denied.sent).toHaveLength(1);
  expect(denied.sent[0]).not.toHaveProperty("claim_ownership");
  expect(denied.updates).toEqual([]);
});

it("shows incarnation conflicts on other operations without retargeting", async () => {
  fetcher.mockResolvedValue(Response.json({ error: { code: "incarnation_conflict" } }, { status: 409 }));
  await expect(renameCentralServer({ ...server, registration_epoch: "stale" }, "new")).rejects.toMatchObject({ code: "incarnation_conflict" });
  expect(fetcher).toHaveBeenCalledTimes(1);
});
