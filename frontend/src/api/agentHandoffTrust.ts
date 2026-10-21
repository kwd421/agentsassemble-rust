import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** "다른 사람 AI가 넘긴 일도 묻지 않기" on one agent, as the server owner sees it. */
function trustPath(roomId: string, sessionId: string): string {
  return `/${encodeURIComponent(roomId)}/${encodeURIComponent(sessionId)}`;
}

function trusted(payload: { trusted?: unknown }): boolean {
  if (typeof payload.trusted !== "boolean") throw new Error("설정 응답이 올바르지 않아요.");
  return payload.trusted;
}

export async function fetchAgentHandoffTrust(roomId: string, sessionId: string, serverId?: string): Promise<boolean> {
  const host = resolvePersonaHost(serverId);
  const suffix = trustPath(roomId, sessionId);
  return trusted(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-handoff-trust${suffix}`, { method: "GET" })).json() as { trusted?: unknown }
    : await fetchJsonServerOperator<{ trusted?: unknown }>(`/api/agent-handoff-trust${suffix}`));
}

export async function setAgentHandoffTrust(roomId: string, sessionId: string, value: boolean, serverId?: string): Promise<boolean> {
  const host = resolvePersonaHost(serverId);
  const suffix = trustPath(roomId, sessionId);
  const body = { trusted: value };
  return trusted(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-handoff-trust${suffix}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
    })).json() as { trusted?: unknown }
    : await postJsonServerOperator<{ trusted?: unknown }>(`/api/agent-handoff-trust${suffix}`, body));
}
