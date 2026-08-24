import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RoomAgentSession, RoomEvent, RoomMember } from "./api";
import type {
  RoomSocketHandle,
  RoomSocketHandlers,
  RoomSocketSnapshot,
} from "./roomSocketClient";
import { useCanonicalRoom } from "./useCanonicalRoom";

const CREATED_AT = "2026-08-25T00:00:00Z";

function event(sequence: number, type: string): RoomEvent {
  return {
    id: `evt-${sequence}`,
    seq: sequence,
    v: 1,
    created_at: CREATED_AT,
    room_id: "general",
    type,
    actor: { participant_id: "operator-local", participant_type: "human" },
    display_name: type === "agent_session_created" ? "OpenCode" : "Operator",
  };
}

function createdAgentEvent(
  sequence = 2,
  runtimeStatus: "stopped" | "starting" = "stopped"
): RoomEvent {
  return {
    ...event(sequence, "agent_session_created"),
    participant_id: "opencode-stopped",
    participant_type: "agent",
    session_id: "opencode-stopped",
    provider_kind: "opencode",
    participant: {
      room_id: "general",
      participant_id: "opencode-stopped",
      display_name: "OpenCode",
      avatar_image_url: "",
      participant_type: "agent",
      status: "detached",
      role: "agent",
      owner_id: "operator-local-user",
      muted: false,
      created_at: CREATED_AT,
      updated_at: CREATED_AT,
    },
    agent_session: {
      room_id: "general",
      session_id: "opencode-stopped",
      participant_id: "opencode-stopped",
      display_name: "OpenCode",
      status: "available",
      runtime_status: runtimeStatus,
      enabled: runtimeStatus === "starting",
      provider_kind: "opencode",
      runtime_kind: "opencode",
      connection_kind: "native_cli_bridge",
      external_owned: false,
      process_ownership: "server",
      model: "opencode/hy3-free",
      reasoning_effort: "",
      service_tier: "",
      variant: "",
      execution_harness: "harness",
      permission_mode: "default",
      max_output_tokens: 0,
      catalog_revision: "catalog-1",
      transport: "jsonl",
      last_seen_event_id: "",
      last_seen_seq: 0,
      last_provider_sync_event_id: "",
      last_provider_sync_seq: 0,
      bootstrap_cutoff_seq: 0,
      turn_count: 0,
      active_turn_id: "",
      turn_phase: "",
      last_error: "",
      last_error_code: "",
      recovery_required: false,
      provider_session_active: false,
      provider_session_reused: false,
      created_at: CREATED_AT,
      updated_at: CREATED_AT,
    },
  } as RoomEvent;
}

function currentSession(): RoomAgentSession {
  return {
    ...createdAgentEvent().agent_session!,
    runtime_status: "idle",
    enabled: true,
  };
}

function currentParticipant(): RoomMember {
  return {
    meeting_id: "general",
    participant_id: "opencode-stopped",
    display_name: "OpenCode",
    participant_type: "unknown",
    role: "agent",
    provider_kind: "opencode",
    connection_kind: "native_cli_bridge",
    status: "joined",
    source: "agent_session",
    created_at: CREATED_AT,
    updated_at: CREATED_AT,
  };
}

function snapshot(
  participants: RoomMember[] = [],
  sessions: RoomAgentSession[] = [],
  events: RoomEvent[] = []
): RoomSocketSnapshot {
  return {
    op: "snapshot",
    stream: "room_events",
    room: { room_id: "general" },
    room_settings: {
      settings_revision: "settings-1",
      label: "General",
      topic: "",
      appearance: {
        banner_preset: "default",
        banner_image_url: "",
        icon_image_url: "",
        icon_label: "G",
        invite_scope: "room",
      },
      conversation_mode: "ordered",
      tool_mode: "chat",
      ordered_exclude_previous_speaker: true,
      channels: [],
    },
    participants,
    agent_sessions: sessions,
    active_turns: [],
    events,
    oldest_seq: events[0]?.seq || 0,
    last_seq: events.at(-1)?.seq || 0,
    has_more_before: true,
    resume_gap: false,
    snapshot_mode: "initial",
    provider_catalog: { status: "ready", catalog_revision: "catalog-1", providers: [] },
    available_providers: [],
    capabilities: { "agent.control": true },
  };
}

function harness(historyBefore: RoomSocketHandle["historyBefore"] = vi.fn()) {
  let handlers: RoomSocketHandlers | undefined;
  const openSocket = vi.fn((_auth, _streams, nextHandlers: RoomSocketHandlers) => {
    handlers = nextHandlers;
    return {
      close: vi.fn(),
      ready: () => true,
      command: vi.fn(),
      say: vi.fn(),
      historyBefore,
    } satisfies RoomSocketHandle;
  });
  return { openSocket, handlers: () => handlers };
}

describe("useCanonicalRoom agent creation projection", () => {
  it("shows stopped creation immediately and retains it across a later start failure", async () => {
    const test = harness();
    const { result } = renderHook(() =>
      useCanonicalRoom({
        serverSurface: TEST_SERVER_PRODUCT_SURFACE,
        roomId: "general",
        auth: { kind: "host", meetingId: "general" },
        openSocket: test.openSocket,
      })
    );
    await waitFor(() => expect(test.openSocket).toHaveBeenCalledOnce());
    act(() => test.handlers()?.onRoomSnapshot?.(snapshot()));
    const created = createdAgentEvent();

    act(() => test.handlers()?.onRoomEvents?.([created]));

    expect(result.current.participants).toEqual([
      expect.objectContaining({ participant_id: "opencode-stopped", status: "detached" }),
    ]);
    expect(result.current.agentSessions).toEqual([
      expect.objectContaining({
        session_id: "opencode-stopped",
        runtime_status: "stopped",
        enabled: false,
      }),
    ]);

    act(() =>
      test.handlers()?.onRoomEvents?.([{
        ...event(3, "agent_session_state"),
        participant_id: "opencode-stopped",
        agent_session: {
          ...created.agent_session!,
          runtime_status: "error",
          last_error: "provider start failed",
          last_error_code: "provider_start_failed",
        },
      }])
    );
    expect(result.current.participants).toHaveLength(1);
    expect(result.current.agentSessions[0]).toMatchObject({
      runtime_status: "error",
      last_error_code: "provider_start_failed",
    });
  });

  it("projects the exact starting creation committed before provider launch", async () => {
    const test = harness();
    const { result } = renderHook(() =>
      useCanonicalRoom({
        serverSurface: TEST_SERVER_PRODUCT_SURFACE,
        roomId: "general",
        auth: { kind: "host", meetingId: "general" },
        openSocket: test.openSocket,
      })
    );
    await waitFor(() => expect(test.openSocket).toHaveBeenCalledOnce());
    act(() => test.handlers()?.onRoomSnapshot?.(snapshot()));

    act(() => test.handlers()?.onRoomEvents?.([createdAgentEvent(2, "starting")]));

    expect(result.current.agentSessions).toEqual([
      expect.objectContaining({
        session_id: "opencode-stopped",
        runtime_status: "starting",
        enabled: true,
      }),
    ]);
  });

  it("uses resume snapshot arrays as the sole current roster and session authority", async () => {
    const test = harness();
    const { result } = renderHook(() =>
      useCanonicalRoom({
        serverSurface: TEST_SERVER_PRODUCT_SURFACE,
        roomId: "general",
        auth: { kind: "host", meetingId: "general" },
        openSocket: test.openSocket,
      })
    );
    await waitFor(() => expect(test.openSocket).toHaveBeenCalledOnce());
    act(() =>
      test.handlers()?.onRoomSnapshot?.(
        snapshot([currentParticipant()], [currentSession()], [event(3, "message_final")])
      )
    );
    const resumed = snapshot(
      [currentParticipant()],
      [currentSession()],
      [createdAgentEvent(2)]
    );
    resumed.snapshot_mode = "resume";

    act(() => test.handlers()?.onRoomSnapshot?.(resumed));

    expect(result.current.participants[0].status).toBe("joined");
    expect(result.current.agentSessions[0]).toMatchObject({
      runtime_status: "idle",
      enabled: true,
    });
  });

  it("does not let paged history overwrite the current roster or session state", async () => {
    const historyBefore = vi.fn().mockResolvedValue({
      events: [createdAgentEvent()],
      oldest_seq: 2,
      last_seq: 2,
      has_more_before: false,
    });
    const test = harness(historyBefore);
    const { result } = renderHook(() =>
      useCanonicalRoom({
        serverSurface: TEST_SERVER_PRODUCT_SURFACE,
        roomId: "general",
        auth: { kind: "host", meetingId: "general" },
        openSocket: test.openSocket,
      })
    );
    await waitFor(() => expect(test.openSocket).toHaveBeenCalledOnce());
    act(() =>
      test.handlers()?.onRoomSnapshot?.(
        snapshot([currentParticipant()], [currentSession()], [event(3, "message_final")])
      )
    );

    await act(async () => {
      await result.current.loadHistory(3);
    });

    expect(result.current.participants[0].status).toBe("joined");
    expect(result.current.agentSessions[0].runtime_status).toBe("idle");
  });
});
