import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** Characters an owner may give one AI; the server enforces the same limit. */
export const MAX_AGENT_INSTRUCTIONS_CHARS = 4000;

function instructionsPath(roomId: string, participantId: string): string {
  return `/${encodeURIComponent(roomId)}/${encodeURIComponent(participantId)}`;
}

function instructions(payload: { instructions?: unknown }): string {
  if (typeof payload.instructions !== "string") throw new Error("지침 응답이 올바르지 않아요.");
  return payload.instructions;
}

/** The owner's own instructions for one AI, app AI or external AI. */
export async function fetchAgentInstructions(roomId: string, participantId: string, remoteServerId: string | null): Promise<string> {
  // null is this computer's server; a remote room names its own server.
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = instructionsPath(roomId, participantId);
  return instructions(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-instructions${suffix}`, { method: "GET" })).json() as { instructions?: unknown }
    : await fetchJsonServerOperator<{ instructions?: unknown }>(`/api/agent-instructions${suffix}`));
}

export async function saveAgentInstructions(roomId: string, participantId: string, text: string, remoteServerId: string | null): Promise<string> {
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = instructionsPath(roomId, participantId);
  const body = { instructions: text };
  return instructions(host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-instructions${suffix}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
    })).json() as { instructions?: unknown }
    : await postJsonServerOperator<{ instructions?: unknown }>(`/api/agent-instructions${suffix}`, body));
}
