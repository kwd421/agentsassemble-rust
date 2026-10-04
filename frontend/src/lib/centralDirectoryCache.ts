import type { CentralServer } from "./centralIdentity";

// Presentation only. No endpoint, host proof or credential can cross this boundary.
export type CentralServerDisplay = Pick<CentralServer, "server_id" | "alias" | "icon" | "host_os" | "relation">;
const KEY = "agentsassemble.centralDirectoryDisplay.v1";
export function clearCentralDirectoryCache() { localStorage.removeItem(KEY); }
export function saveCentralDirectoryCache(personId: string, servers: CentralServerDisplay[]) {
  localStorage.setItem(KEY, JSON.stringify({ personId, servers: servers.map(project) }));
}
function project(server: CentralServerDisplay): CentralServerDisplay {
  return { server_id: server.server_id, alias: server.alias, icon: server.icon || "",
    host_os: server.host_os, relation: server.relation };
}
export function loadCentralDirectoryCache(personId: string): CentralServerDisplay[] {
  const value = JSON.parse(localStorage.getItem(KEY) || "null");
  if (!value) return [];
  if (value.personId !== personId) { clearCentralDirectoryCache(); return []; }
  if (!Array.isArray(value.servers) || !value.servers.every((s: CentralServerDisplay) =>
    s && typeof s.server_id === "string" && typeof s.alias === "string" &&
    typeof s.icon === "string" && ["owner", "bookmark"].includes(s.relation) &&
    [null, "macos", "windows", "linux", "other"].includes(s.host_os))) {
    throw new Error("저장된 서버 목록을 읽지 못했어요.");
  }
  return value.servers.map(project);
}
