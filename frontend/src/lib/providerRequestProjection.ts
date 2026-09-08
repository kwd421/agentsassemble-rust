import type { RoomEvent } from "../api";
import type { PendingProviderRequest } from "../types/generated/PendingProviderRequest";
import { parseProviderRequest } from "./providerRequestContract";

export function providerRequestEventIsValid(event: RoomEvent): boolean {
  if (!["provider_request_opened", "provider_request_resolving", "provider_request_closed"].includes(event.type)) return true;
  if (event.visibility !== "owner" || typeof event.owner_id !== "string" || !event.owner_id) return false;
  if (event.type === "provider_request_opened") {
    try {
      parseProviderRequest(event.provider_request);
      return typeof event.session_id === "string" && Boolean(event.session_id) &&
        typeof event.expires_at === "string" && Number.isFinite(Date.parse(event.expires_at));
    } catch { return false; }
  }
  return typeof event.provider_request_id === "string" && Boolean(event.provider_request_id) &&
    (event.type !== "provider_request_closed" || ["resolved", "failed", "cancelled", "expired"].includes(String(event.state)));
}

export function applyProviderRequestEvents(
  previous: PendingProviderRequest[], events: RoomEvent[], ownerId: string
): PendingProviderRequest[] {
  const pending = new Map(previous.map((entry) => [entry.request.provider_request_id, entry]));
  for (const event of events) {
    if (event.owner_id !== ownerId) continue;
    if (event.type === "provider_request_opened") {
      const request = parseProviderRequest(event.provider_request);
      pending.set(request.provider_request_id, {
        request, session_id: String(event.session_id), expires_at: String(event.expires_at), state: "open",
      });
    } else if (event.type === "provider_request_resolving") {
      const id = String(event.provider_request_id);
      const current = pending.get(id);
      if (current) pending.set(id, { ...current, state: "resolving" });
    } else if (event.type === "provider_request_closed") {
      pending.delete(String(event.provider_request_id));
    }
  }
  return [...pending.values()];
}
