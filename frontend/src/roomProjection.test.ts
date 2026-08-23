import { describe, expect, it } from "vitest";
import type { RoomAgentSession, RoomEvent } from "./api";
import {
  acceptSnapshotEvents,
  projectAgentSessionEvents,
  visibleTimelineEvents,
} from "./roomProjection";

function event(id: string, seq: number): RoomEvent {
  return {
    v: 1,
    id,
    seq,
    created_at: "2026-08-22T00:00:00Z",
    room_id: "general",
    type: "message_final",
    actor: { participant_id: "operator-local", participant_type: "human" },
    content: id,
  };
}

describe("room snapshot projection", () => {
  it("keeps the existing projection when an empty resume confirms its cursor", () => {
    expect(acceptSnapshotEvents([event("one", 1)], [], "resume")).toEqual([event("one", 1)]);
  });

  it("replaces stale state for an authoritative initial snapshot", () => {
    expect(acceptSnapshotEvents([event("stale", 1)], [event("fresh", 2)], "initial"))
      .toEqual([event("fresh", 2)]);
  });

  it("projects public Agent Session state separately from the visible timeline", () => {
    const session = {
      room_id: "general",
      session_id: "codex-session-1",
      participant_id: "codex-session-1",
      display_name: "Terra",
      runtime_status: "busy",
      process_ownership: "server",
      external_owned: false,
      provider_kind: "codex_live_session",
      model: "gpt-5.6-terra",
    } as RoomAgentSession;
    const state = {
      ...event("state", 2),
      type: "agent_session_state",
      agent_session: session,
    } as RoomEvent;
    const internal = { ...event("turn", 3), type: "turn_started" };
    const message = event("answer", 4);

    expect(projectAgentSessionEvents([], [state, internal, message])).toEqual([session]);
    expect(visibleTimelineEvents([state, internal, message])).toEqual([message]);
  });

  it("rejects private Agent Session authority from event projection", () => {
    const leaked = {
      ...event("state", 2),
      type: "agent_session_state",
      agent_session: {
        room_id: "general",
        session_id: "codex-session-1",
        participant_id: "codex-session-1",
        display_name: "Terra",
        runtime_status: "busy",
        process_ownership: "server",
        external_owned: false,
        provider_kind: "codex_live_session",
        model: "gpt-5.6-terra",
        runtime_handle_id: "private-runtime",
      },
    } as RoomEvent;
    expect(projectAgentSessionEvents([], [leaked])).toEqual([]);
  });
});
