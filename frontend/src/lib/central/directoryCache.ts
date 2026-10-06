import type { CentralServer } from "./identity";

// Presentation only. No endpoint, host proof or credential can cross this boundary.
export type CentralServerDisplay = Pick<CentralServer, "server_id" | "registration_epoch" | "alias" | "icon" | "host_os" | "relation" | "default_name" | "name_is_default">;
const KEY = "agentsassemble.centralDirectoryDisplay.v1";
export function clearCentralDirectoryCache() { localStorage.removeItem(KEY); }
export function saveCentralDirectoryCache(personId: string, servers: CentralServerDisplay[]) {
  localStorage.setItem(KEY, JSON.stringify({ personId, servers: servers.map(project) }));
}
function project(server: CentralServerDisplay): CentralServerDisplay {
  if (server.relation === "member") return {server_id:server.server_id,relation:"member",alias:server.alias,icon:server.icon || "",registration_epoch:server.registration_epoch};
  return { server_id: server.server_id, alias: server.alias, icon: server.icon || "",
    host_os: server.host_os, relation: server.relation,
    ...(server.default_name === undefined ? {} : { default_name: server.default_name }),
    ...(server.name_is_default === undefined ? {} : { name_is_default: server.name_is_default }),
    ...(server.registration_epoch === undefined ? {} : { registration_epoch: server.registration_epoch }) };
}
export function loadCentralDirectoryCache(personId: string): CentralServerDisplay[] {
  const value = JSON.parse(localStorage.getItem(KEY) || "null");
  if (!value) return [];
  if (value.personId !== personId) { clearCentralDirectoryCache(); return []; }
  if (!Array.isArray(value.servers) || !value.servers.every((s: CentralServerDisplay) =>
    s && typeof s.server_id === "string" && typeof s.alias === "string" &&
    (s.default_name === undefined || typeof s.default_name === "string") &&
    (s.name_is_default === undefined || typeof s.name_is_default === "boolean") &&
    typeof s.icon === "string" && ["owner", "bookmark", "member"].includes(s.relation) &&
    (s.relation === "member" || [null, "macos", "windows", "linux", "other"].includes(s.host_os ?? null)))) {
    throw new Error("저장된 서버 목록을 읽지 못했어요.");
  }
  return value.servers.map(project);
}
