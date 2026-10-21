import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** The people whose AIs one agent trusts, as the server owner sees it. */
function trustPath(roomId: string, sessionId: string): string {
  return `/${encodeURIComponent(roomId)}/${encodeURIComponent(sessionId)}`;
}

function owners(payload: { trusted_owners?: unknown }): string[] {
  const list = payload.trusted_owners;
  if (!Array.isArray(list) || !list.every((id) => typeof id === "string")) throw new Error("설정 응답이 올바르지 않아요.");
  return list;
}

export async function fetchAgentTrustedOwners(roomId: string, sessionId: string, remoteServerId: string | null): Promise<string[]> {
  // null is this computer's server; a remote room names its own server.
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = trustPath(roomId, sessionId);
  return owners(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-handoff-trust${suffix}`, { method: "GET" })).json() as { trusted_owners?: unknown }
    : await fetchJsonServerOperator<{ trusted_owners?: unknown }>(`/api/agent-handoff-trust${suffix}`));
}

export async function setAgentTrustedOwners(roomId: string, sessionId: string, trusted: string[], remoteServerId: string | null): Promise<string[]> {
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = trustPath(roomId, sessionId);
  const body = { trusted_owners: trusted };
  return owners(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-handoff-trust${suffix}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
    })).json() as { trusted_owners?: unknown }
    : await postJsonServerOperator<{ trusted_owners?: unknown }>(`/api/agent-handoff-trust${suffix}`, body));
}
