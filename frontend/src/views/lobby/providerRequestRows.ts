import type { LobbyEvent, RoomAgentSession } from "../../api";
import type { PendingProviderRequest } from "../../types/generated/PendingProviderRequest";

/** Pending owner requests remain answerable even when their opening event was paged out. */
export function withPendingProviderRequests(events: LobbyEvent[], requests: PendingProviderRequest[], sessions: RoomAgentSession[]): LobbyEvent[] {
  const result = [...events];
  const visible = new Set(events.map((event) => event.provider_request_id).filter(Boolean));
  for (const entry of requests) {
    if (visible.has(entry.request.provider_request_id)) continue;
    const session = sessions.find((item) => item.session_id === entry.session_id);
    if (!session) continue;
    const row: LobbyEvent = {
      id: `provider-request:${entry.request.provider_request_id}`, kind: "provider_request",
      name: session.display_name, actor_id: session.participant_id, actor_type: "agent",
      provider_kind: session.provider_kind, message: "", side: "other",
      created_at: new Date(Date.parse(entry.expires_at) - entry.request.timeout_seconds * 1000).toISOString(),
      provider_request_id: entry.request.provider_request_id, provider_request_title: entry.request.title,
      provider_request_state: entry.state,
    };
    const next = result.findIndex((event) => Date.parse(event.created_at) > Date.parse(row.created_at));
    result.splice(next < 0 ? result.length : next, 0, row);
  }
  return result;
}
