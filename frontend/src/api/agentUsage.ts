import { fetchJsonServerOperator } from "./http";
import { remoteOwnerRoute, resolvePersonaHost } from "./personas";

/** An API AI's model-use totals, as the server owner sees them. */
export type AgentUsageTotals = {
  turns: number; steps: number; prompt_tokens: number; cached_prompt_tokens: number;
  completion_tokens: number; last_prompt_tokens: number; context_tokens: number; model_ms: number;
};

const FIELDS: (keyof AgentUsageTotals)[] = ["turns", "steps", "prompt_tokens", "cached_prompt_tokens",
  "completion_tokens", "last_prompt_tokens", "context_tokens", "model_ms"];

export async function fetchAgentUsage(roomId: string, sessionId: string, remoteServerId: string | null): Promise<AgentUsageTotals> {
  // null is this computer's server; a remote room names its own server.
  const host = remoteServerId === null ? null : resolvePersonaHost(remoteServerId);
  const suffix = `/${encodeURIComponent(roomId)}/${encodeURIComponent(sessionId)}`;
  const payload = host
    ? await (await remoteOwnerRoute(host, `/api/central-owner/agent-usage${suffix}`, { method: "GET" })).json() as { usage?: Record<string, unknown> }
    : await fetchJsonServerOperator<{ usage?: Record<string, unknown> }>(`/api/agent-usage${suffix}`);
  const usage = payload.usage;
  if (!usage || !FIELDS.every((field) => Number.isSafeInteger(usage[field]) && Number(usage[field]) >= 0)) {
    throw new Error("사용량 응답이 올바르지 않아요.");
  }
  return Object.fromEntries(FIELDS.map((field) => [field, Number(usage[field])])) as AgentUsageTotals;
}
