import { deleteJsonServerOperator, fetchJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** One memory piece of an API agent, as the server owner sees it. */
export type AgentMemoryPiece = { from_seq: number; to_seq: number; summary: string; created_at: number };

function memoryPath(roomId: string, sessionId: string, fromSeq?: number): string {
  const base = `/${encodeURIComponent(roomId)}/${encodeURIComponent(sessionId)}`;
  return fromSeq === undefined ? base : `${base}/${fromSeq}`;
}

export async function fetchAgentMemory(roomId: string, sessionId: string, serverId?: string): Promise<AgentMemoryPiece[]> {
  const host = resolvePersonaHost(serverId);
  const suffix = memoryPath(roomId, sessionId);
  const payload = host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-memory${suffix}`, { method: "GET" })).json() as { pieces?: AgentMemoryPiece[] }
    : await fetchJsonServerOperator<{ pieces?: AgentMemoryPiece[] }>(`/api/agent-memory${suffix}`);
  if (!Array.isArray(payload.pieces)) throw new Error("기억 응답이 올바르지 않아요.");
  return payload.pieces;
}

export async function deleteAgentMemory(roomId: string, sessionId: string, fromSeq: number, serverId?: string): Promise<void> {
  const host = resolvePersonaHost(serverId);
  const suffix = memoryPath(roomId, sessionId, fromSeq);
  if (host) await remoteOwnerRoute(host, `/api/central-owner/agent-memory${suffix}`, { method: "DELETE" });
  else await deleteJsonServerOperator(`/api/agent-memory${suffix}`);
}
