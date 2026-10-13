import { useSyncExternalStore } from "react";
import type { RoomEvent } from "../api";
import type { RoomSocketHandle } from "../roomSocketTypes";
import type { DelegatedActionRequest } from "../types/generated/DelegatedActionRequest";
import type { DelegatedActionDecisionAck } from "../types/generated/DelegatedActionDecisionAck";
import type { DecideDelegatedAction } from "../types/generated/DecideDelegatedAction";
import { assertExactKeys, strictRecord } from "./strictJsonContract";

const states = ["pending", "declined", "expired", "cancelled", "executing", "succeeded", "failed"];
export function parseDelegatedActionRequest(value: unknown): DelegatedActionRequest {
  const r = strictRecord(value, "delegated action request");
  assertExactKeys(r, ["confirmation_id", "owner_participant_id", "owner_display_name", "delegate_participant_id", "delegate_display_name", "action", "target_name", "consequences", "created_at", "expires_at", "state"], "delegated action request");
  for (const key of ["confirmation_id", "owner_participant_id", "owner_display_name", "delegate_participant_id", "delegate_display_name", "target_name", "consequences"]) {
    if (typeof r[key] !== "string" || !r[key]) throw new Error("Invalid delegated action identity");
  }
  if (!states.includes(String(r.state)) || typeof r.created_at !== "string" || !Number.isFinite(Date.parse(r.created_at)) || typeof r.expires_at !== "string" || !Number.isFinite(Date.parse(r.expires_at))) throw new Error("Invalid confirmation lifetime");
  const action = strictRecord(r.action, "confirmed action");
  assertExactKeys(action, ["action", "request_id", "channel_id", "expected_settings_revision"], "confirmed action");
  if (action.action !== "delete_text_channel" || ["request_id", "channel_id", "expected_settings_revision"].some((k) => typeof action[k] !== "string" || !action[k])) throw new Error("Invalid confirmed action");
  return r as unknown as DelegatedActionRequest;
}
export function delegatedActionRequestsAreValid(value: unknown): boolean {
  try { if (!Array.isArray(value)) return false; value.forEach(parseDelegatedActionRequest); return true; } catch { return false; }
}
export function delegatedActionEventIsValid(event: RoomEvent): boolean {
  if (!["delegated_action_requested", "delegated_action_updated"].includes(event.type)) return true;
  try { const request = parseDelegatedActionRequest(event.delegated_action_request); return event.visibility === "owner" && event.owner_id === request.owner_participant_id; } catch { return false; }
}
export function applyDelegatedActionEvents(previous: readonly DelegatedActionRequest[], events: RoomEvent[]): DelegatedActionRequest[] {
  const requests = new Map(previous.map((r) => [r.confirmation_id, r]));
  for (const e of events) {
    if (!["delegated_action_requested", "delegated_action_updated"].includes(e.type)) continue;
    const request = parseDelegatedActionRequest(e.delegated_action_request);
    if (request.state === "pending") requests.set(request.confirmation_id, request);
    else requests.delete(request.confirmation_id);
  }
  return [...requests.values()];
}
export function delegatedActionAckIsValid(payload: Record<string, unknown>, result: Record<string, unknown>): boolean {
  try {
    assertExactKeys(result, ["confirmation_id", "state", "receipt"], "delegated decision ACK");
    return result.confirmation_id === payload.confirmation_id && states.includes(String(result.state)) && typeof result.receipt === "object" && result.receipt !== null;
  } catch { return false; }
}

const empty: readonly DelegatedActionRequest[] = [];
const stores = new WeakMap<RoomSocketHandle, ReturnType<typeof createDelegatedActionProjection>>();
export function createDelegatedActionProjection() {
  let requests: readonly DelegatedActionRequest[] = empty;
  const listeners = new Set<() => void>();
  return {
    read: () => requests,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    replace: (next: readonly DelegatedActionRequest[]) => { requests = next; listeners.forEach((listener) => listener()); },
  };
}
export function registerDelegatedActionProjection(socket: RoomSocketHandle, store: ReturnType<typeof createDelegatedActionProjection>) { stores.set(socket, store); }
export function useDelegatedActionRequests(socket: RoomSocketHandle | null): readonly DelegatedActionRequest[] {
  const store = socket ? stores.get(socket) : undefined;
  return useSyncExternalStore(store?.subscribe ?? (() => () => {}), store?.read ?? (() => empty));
}
export function readDelegatedActionRequests(socket: RoomSocketHandle): readonly DelegatedActionRequest[] { return stores.get(socket)?.read() ?? empty; }
export async function decideDelegatedAction(socket: RoomSocketHandle, decision: DecideDelegatedAction): Promise<DelegatedActionDecisionAck> {
  const ack = await socket.command("delegated_action.decide", { ...decision });
  return ack.result as unknown as DelegatedActionDecisionAck;
}
