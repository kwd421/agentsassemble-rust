import { afterEach, expect, it } from "vitest";
import { clearCentralSession, saveSession } from "./identity";
import { loadCentralDirectoryCache, saveCentralDirectoryCache } from "./directoryCache";
const person = { person_id: "account-one", display_name: "Name", identity_kind: "google" as const };
const session = { token: "secret-token", device_id: "device", expires_at: 9_999_999_999 };
const server = { server_id: "server", alias: "My Mac", icon: "/v1/servers/server/icon/version.png",
  host_os: "macos" as const, relation: "owner" as const,
  endpoint: { origin: "https://private.example" }, grant_token: "private-grant", recovery_code: "private-code" };
afterEach(() => localStorage.clear());
it("persists only display fields, with no connect authority or secrets", () => {
  saveCentralDirectoryCache(person.person_id, [server]);
  const rows = loadCentralDirectoryCache(person.person_id);
  expect(rows).toEqual([{ server_id: "server", alias: "My Mac", icon: server.icon, host_os: "macos", relation: "owner" }]);
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).not.toMatch(/private|grant|recovery|token|endpoint/);
});
it("clears another account's display cache when login succeeds and on logout", () => {
  saveSession({ person, session });
  saveCentralDirectoryCache(person.person_id, [server]);
  saveSession({ person: { ...person, person_id: "account-two" }, session });
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).toBeNull();
  saveCentralDirectoryCache("account-two", [server]);
  clearCentralSession();
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).toBeNull();
});
it("does not display a cache belonging to another account", () => {
  saveCentralDirectoryCache("account-one", [server]);
  expect(loadCentralDirectoryCache("account-two")).toEqual([]);
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).toBeNull();
});

it("deletes the unscoped legacy directory on every account transition", () => {
  for (const account of ["account-one", "account-two", "account-two"]) {
    localStorage.setItem("agentsassemble.centralServers.v1", JSON.stringify([
      { ...server, host_public_key_jwk: { x: "old-host-key" } },
    ]));
    saveSession({ person: { ...person, person_id: account }, session });
    expect(localStorage.getItem("agentsassemble.centralServers.v1")).toBeNull();
  }
});
