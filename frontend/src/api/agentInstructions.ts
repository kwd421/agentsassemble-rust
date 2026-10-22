import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** Characters an owner may give one AI; the server enforces the same limit. */
export const MAX_AGENT_INSTRUCTIONS_CHARS = 4000;

/** The owner's own settings for one AI, app AI or external AI. */
export type AgentOwnerSettings = { instructions: string; mention_only: boolean };

function settingsPath(roomId: string, participantId: string): string {
  return `/${encodeURIComponent(roomId)}/${encodeURIComponent(participantId)}`;
}

function settings(payload: { instructions?: unknown; mention_only?: unknown }): AgentOwnerSettings {
  if (typeof payload.instructions !== "string") throw new Error("지침 응답이 올바르지 않아요.");
  // Hosts from before the switch answer without it; they always follow the room.
  return { instructions: payload.instructions, mention_only: payload.mention_only === true };
}

export async function fetchAgentOwnerSettings(roomId: string, participantId: string, remoteServerId: string | null): Promise<AgentOwnerSettings> {
  // null is this computer's server; a remote room names its own server.
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = settingsPath(roomId, participantId);
  return settings(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-instructions${suffix}`, { method: "GET" })).json() as { instructions?: unknown }
    : await fetchJsonServerOperator<{ instructions?: unknown }>(`/api/agent-instructions${suffix}`));
}

export async function saveAgentOwnerSettings(roomId: string, participantId: string, change: Partial<AgentOwnerSettings>, remoteServerId: string | null): Promise<AgentOwnerSettings> {
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = settingsPath(roomId, participantId);
  return settings(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-instructions${suffix}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(change),
    })).json() as { instructions?: unknown }
    : await postJsonServerOperator<{ instructions?: unknown }>(`/api/agent-instructions${suffix}`, change));
}
