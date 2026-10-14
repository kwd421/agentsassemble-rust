import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { handshakeFrames, openHarness, flushPromises } from "./test/roomSocketHarness";
import { decideDelegatedAction, parseDelegatedActionRequest, readDelegatedActionRequests } from "./lib/delegatedActionProjection";
import type { DelegatedActionRequest } from "./types/generated/DelegatedActionRequest";

const request: DelegatedActionRequest = {
  confirmation_id: "8b6be8f4-7315-4b6c-b4b6-ff12f57e69b4", owner_participant_id: "operator-local", owner_display_name: "Owner",
  delegate_participant_id: "ai", delegate_display_name: "Director", action: { action: "delete_text_channel", request_id: "8b6be8f4-7315-4b6c-b4b6-ff12f57e69b5", channel_id: "ctarget", expected_settings_revision: "rev" },
  target_name: "Planning", consequences: "Deletes messages", created_at: "2026-10-13T00:00:00Z", expires_at: "2026-10-13T00:05:00Z", state: "pending",
};
describe("owner confirmation socket projection", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  it("hydrates pending requests, applies terminal events and sends the exact decision identity", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    const socket = sockets[0];
    socket.open();
    const frames = handshakeFrames(0, 0);
    socket.receive(frames.receipt); socket.receive(frames.catalog); socket.receive(frames.requestsEnd);
    socket.receive({ ...frames.snap, delegated_action_requests: [request] });
    expect(readDelegatedActionRequests(handle)).toEqual([request]);
    const decided = decideDelegatedAction(handle, { confirmation_id: request.confirmation_id, decision: "decline" });
    expect(socket.sent.at(-1)).toMatchObject({ op: "command", request_id: request.confirmation_id, action: "delegated_action.decide", payload: { confirmation_id: request.confirmation_id, decision: "decline" } });
    socket.receive({ op: "ack", request_id: request.confirmation_id, accepted: true, resolution: "committed", action: "delegated_action.decide", result: { confirmation_id: request.confirmation_id, state: "declined", receipt: { status: "rejected", code: "confirmation_declined" } } });
    await expect(decided).resolves.toMatchObject({ state: "declined" });
    socket.receive({ op: "event", stream: "room_events", latest_seq: 1, events: [{ v: 1, id: "event", room_id: "general", seq: 1, created_at: request.created_at, type: "delegated_action_updated", actor: { participant_id: "ai", participant_type: "agent" }, visibility: "owner", owner_id: request.owner_participant_id, delegated_action_request: { ...request, state: "declined" } }] });
    expect(readDelegatedActionRequests(handle)).toEqual([]);
    handle.close();
  });
  it("parses a bound agent-creation request so the owner card can render it", () => {
    const created: DelegatedActionRequest = {
      ...request,
      action: {
        action: "create_agent", request_id: request.confirmation_id,
        params: { request_id: request.confirmation_id, provider_id: "codex", catalog_revision: "catalog-1", display_name: "Newcomer", workspace: "/tmp/room", permission_mode: "workspace_write" },
        draft: {
          agent_id: "codex-new", display_name: "Newcomer", provider_kind: "codex_live_session", runtime_kind: "live_cli", connection_kind: "native_cli_bridge",
          executable: "/bin/codex", executable_identity: "exe", workspace: "/tmp/room", workspace_identity: "ws", provider_endpoint: "", model: "gpt-5.6-terra",
          reasoning_effort: "medium", service_tier: "default", variant: "", execution_harness: "builtin", permission_mode: "workspace_write", max_output_tokens: 0,
          catalog_revision: "catalog-1", persona_card_id: "", runtime_profile_key: "profile", transport: "stdio_jsonl",
        },
      },
      target_name: "Newcomer",
    };
    expect(parseDelegatedActionRequest(structuredClone(created))).toEqual(created);
    const missing = structuredClone(created);
    if (missing.action.action === "create_agent") delete (missing.action.draft as Record<string, unknown>).model;
    expect(() => parseDelegatedActionRequest(missing)).toThrow();
  });
  it("parses a bound agent-reconfiguration request so the owner card can render the changed profile", () => {
    const reconfigured: DelegatedActionRequest = {
      ...request,
      action: {
        action: "reconfigure_agent", request_id: request.confirmation_id, agent_id: "codex-new",
        params: { request_id: request.confirmation_id, agent_id: "codex-new", catalog_revision: "catalog-1", model: "gpt-5.6-terra", permission_mode: "workspace_write" },
        expected_configuration_revision: "profile",
        draft: {
          agent_id: "codex-new", display_name: "Newcomer", provider_kind: "codex_live_session", runtime_kind: "live_cli", connection_kind: "native_cli_bridge",
          executable: "/bin/codex", executable_identity: "exe", workspace: "/tmp/room", workspace_identity: "ws", provider_endpoint: "", model: "gpt-5.6-terra",
          reasoning_effort: "medium", service_tier: "default", variant: "", execution_harness: "builtin", permission_mode: "workspace_write", max_output_tokens: 0,
          catalog_revision: "catalog-1", persona_card_id: "", runtime_profile_key: "profile-next", transport: "stdio_jsonl",
        },
      },
      target_name: "Newcomer",
    };
    expect(parseDelegatedActionRequest(structuredClone(reconfigured))).toEqual(reconfigured);
    const changed = structuredClone(reconfigured);
    if (changed.action.action === "reconfigure_agent") delete (changed.action as Record<string, unknown>).expected_configuration_revision;
    expect(() => parseDelegatedActionRequest(changed)).toThrow();
  });
  it("parses a bound agent-removal request so the owner card can render who is removed", () => {
    const removed: DelegatedActionRequest = {
      ...request,
      action: {
        action: "remove_agent", request_id: request.confirmation_id, agent_id: "codex-old",
        params: { request_id: request.confirmation_id, agent_id: "codex-old" },
        expected_created_at: "2026-10-13T00:00:00Z",
      },
      target_name: "Oldtimer",
    };
    expect(parseDelegatedActionRequest(structuredClone(removed))).toEqual(removed);
    const changed = structuredClone(removed);
    if (changed.action.action === "remove_agent") delete (changed.action as Record<string, unknown>).expected_created_at;
    expect(() => parseDelegatedActionRequest(changed)).toThrow();
  });
  it("parses a bound participant-kick request so the owner card can render who is removed and the consequence", () => {
    const kicked: DelegatedActionRequest = {
      ...request,
      action: {
        action: "kick_participant", request_id: request.confirmation_id, participant_id: "guest-1",
        params: { request_id: request.confirmation_id, participant_id: "guest-1" },
        expected_created_at: "2026-10-13T00:00:00Z",
      },
      target_name: "Guest", consequences: "Removes the person Guest from the room",
    };
    expect(parseDelegatedActionRequest(structuredClone(kicked))).toEqual(kicked);
    const changed = structuredClone(kicked);
    if (changed.action.action === "kick_participant") delete (changed.action.params as Record<string, unknown>).participant_id;
    expect(() => parseDelegatedActionRequest(changed)).toThrow();
  });
});
