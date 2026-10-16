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
  if (action.action === "delete_text_channel") {
    assertExactKeys(action, ["action", "request_id", "channel_id"], "confirmed action", ["expected_settings_revision"]);
    if (["request_id", "channel_id"].some((k) => typeof action[k] !== "string" || !action[k])) throw new Error("Invalid confirmed channel action");
    if (action.expected_settings_revision !== undefined && (typeof action.expected_settings_revision !== "string" || !action.expected_settings_revision)) throw new Error("Invalid confirmed channel action");
  } else if (action.action === "start_agent") {
    assertExactKeys(action, ["action", "request_id", "agent_id", "configuration", "expected_configuration_revision", "expected_created_at"], "confirmed agent start");
    if (["request_id", "agent_id"].some((k) => typeof action[k] !== "string" || !action[k]) || typeof action.expected_configuration_revision !== "string" || typeof action.expected_created_at !== "string" || !Number.isFinite(Date.parse(action.expected_created_at))) throw new Error("Invalid confirmed agent identity");
    const config = strictRecord(action.configuration, "confirmed agent configuration");
    const strings = ["provider_kind", "runtime_kind", "connection_kind", "model", "reasoning_effort", "service_tier", "variant", "execution_harness", "permission_mode", "catalog_revision", "persona_card_id", "transport", "working_folder"];
    assertExactKeys(config, [...strings, "max_output_tokens"], "confirmed agent configuration");
    if (strings.some((k) => typeof config[k] !== "string") || !Number.isSafeInteger(config.max_output_tokens) || Number(config.max_output_tokens) < 0) throw new Error("Invalid confirmed agent configuration");
  } else if (action.action === "create_agent") {
    assertExactKeys(action, ["action", "request_id", "params", "draft"], "confirmed agent creation");
    if (typeof action.request_id !== "string" || !action.request_id) throw new Error("Invalid confirmed creation identity");
    const params = strictRecord(action.params, "confirmed creation params");
    const required = ["request_id", "provider_id", "catalog_revision", "display_name", "workspace"];
    const optional = ["model", "reasoning_effort", "service_tier", "variant", "permission_mode", "persona_card_id", "provider_endpoint", "max_output_tokens"];
    if (required.some((k) => typeof params[k] !== "string" || !params[k])) throw new Error("Invalid confirmed creation params");
    if (Object.keys(params).some((k) => ![...required, ...optional].includes(k))) throw new Error("Invalid confirmed creation params");
    for (const key of ["model", "reasoning_effort", "service_tier", "variant", "permission_mode", "persona_card_id", "provider_endpoint"]) {
      if (params[key] !== undefined && typeof params[key] !== "string") throw new Error("Invalid confirmed creation params");
    }
    if (params.max_output_tokens !== undefined && (!Number.isSafeInteger(params.max_output_tokens) || Number(params.max_output_tokens) < 0)) throw new Error("Invalid confirmed creation params");
    assertAgentDraft(action.draft, "confirmed creation draft");
  } else if (action.action === "reconfigure_agent") {
    assertExactKeys(action, ["action", "request_id", "agent_id", "params", "expected_configuration_revision", "draft"], "confirmed agent reconfiguration");
    if (["request_id", "agent_id", "expected_configuration_revision"].some((k) => typeof action[k] !== "string" || !action[k])) throw new Error("Invalid confirmed reconfiguration identity");
    const params = strictRecord(action.params, "confirmed reconfiguration params");
    const required = ["request_id", "agent_id", "catalog_revision"];
    const optional = ["model", "reasoning_effort", "permission_mode", "workspace"];
    if (required.some((k) => typeof params[k] !== "string" || !params[k])) throw new Error("Invalid confirmed reconfiguration params");
    if (Object.keys(params).some((k) => ![...required, ...optional].includes(k))) throw new Error("Invalid confirmed reconfiguration params");
    for (const key of ["model", "reasoning_effort", "permission_mode", "workspace"]) {
      if (params[key] !== undefined && typeof params[key] !== "string") throw new Error("Invalid confirmed reconfiguration params");
    }
    assertAgentDraft(action.draft, "confirmed reconfiguration draft");
  } else if (action.action === "remove_agent") {
    assertExactKeys(action, ["action", "request_id", "agent_id", "params", "expected_created_at"], "confirmed agent removal");
    if (["request_id", "agent_id"].some((k) => typeof action[k] !== "string" || !action[k]) || typeof action.expected_created_at !== "string" || !Number.isFinite(Date.parse(action.expected_created_at))) throw new Error("Invalid confirmed removal identity");
    const params = strictRecord(action.params, "confirmed removal params");
    assertExactKeys(params, ["request_id", "agent_id"], "confirmed removal params");
    if (params.request_id !== action.request_id || params.agent_id !== action.agent_id) throw new Error("Invalid confirmed removal params");
  } else if (action.action === "kick_participant") {
    assertExactKeys(action, ["action", "request_id", "participant_id", "params", "expected_created_at", "expected_updated_at"], "confirmed participant kick");
    if (["request_id", "participant_id"].some((k) => typeof action[k] !== "string" || !action[k]) || typeof action.expected_created_at !== "string" || !Number.isFinite(Date.parse(action.expected_created_at)) || typeof action.expected_updated_at !== "string" || !Number.isFinite(Date.parse(action.expected_updated_at))) throw new Error("Invalid confirmed kick identity");
    const params = strictRecord(action.params, "confirmed kick params");
    assertExactKeys(params, ["request_id", "participant_id"], "confirmed kick params");
    if (params.request_id !== action.request_id || params.participant_id !== action.participant_id) throw new Error("Invalid confirmed kick params");
  } else if (action.action === "create_human_invite") {
    assertExactKeys(action, ["action", "request_id", "params"], "confirmed human invite");
    if (typeof action.request_id !== "string" || !action.request_id) throw new Error("Invalid confirmed invite identity");
    const params = strictRecord(action.params, "confirmed invite params");
    assertExactKeys(params, ["request_id", "invite_scope", "ttl_seconds", "max_uses"], "confirmed invite params", ["display_name"]);
    if (params.request_id !== action.request_id) throw new Error("Invalid confirmed invite params");
    if (params.display_name !== undefined && typeof params.display_name !== "string") throw new Error("Invalid confirmed invite params");
    if (params.invite_scope !== "read_write" && params.invite_scope !== "read_only") throw new Error("Invalid confirmed invite scope");
    if (!Number.isSafeInteger(params.ttl_seconds) || Number(params.ttl_seconds) <= 0) throw new Error("Invalid confirmed invite lifetime");
    if (!Number.isSafeInteger(params.max_uses) || Number(params.max_uses) < 0) throw new Error("Invalid confirmed invite uses");
  } else { throw new Error("Unsupported confirmed action"); }
  return r as unknown as DelegatedActionRequest;
}
function assertAgentDraft(value: unknown, label: string) {
  const draft = strictRecord(value, label);
  const strings = ["agent_id", "display_name", "provider_kind", "runtime_kind", "connection_kind", "executable", "executable_identity", "workspace", "workspace_identity", "provider_endpoint", "model", "reasoning_effort", "service_tier", "variant", "execution_harness", "permission_mode", "catalog_revision", "persona_card_id", "runtime_profile_key", "transport"];
  assertExactKeys(draft, [...strings, "max_output_tokens"], label);
  if (strings.some((k) => typeof draft[k] !== "string")) throw new Error(`Invalid ${label}`);
  if (["agent_id", "display_name", "provider_kind", "runtime_kind", "model", "permission_mode", "catalog_revision", "runtime_profile_key", "transport"].some((k) => !draft[k])) throw new Error(`Invalid ${label}`);
  if (!Number.isSafeInteger(draft.max_output_tokens) || Number(draft.max_output_tokens) < 0) throw new Error(`Invalid ${label}`);
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
    assertExactKeys(result, ["confirmation_id", "state", "receipt"], "delegated decision ACK", ["invite"]);
    if (result.confirmation_id !== payload.confirmation_id || !states.includes(String(result.state)) || typeof result.receipt !== "object" || result.receipt === null) return false;
    if (result.invite !== undefined) {
      const invite = strictRecord(result.invite, "delegated invite");
      assertExactKeys(invite, ["invite_id", "invite_token", "join_code", "join_url", "display_name", "invite_scope", "max_uses", "expires_at"], "delegated invite");
      if (["invite_id", "invite_token", "join_code", "join_url", "display_name", "invite_scope", "expires_at"].some((key) => typeof invite[key] !== "string" || !invite[key])) return false;
      if (!Number.isSafeInteger(invite.max_uses) || Number(invite.max_uses) < 0) return false;
      if (!Number.isFinite(Date.parse(String(invite.expires_at)))) return false;
    }
    return true;
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
