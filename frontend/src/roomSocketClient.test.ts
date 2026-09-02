import { afterEach, describe, expect, it, vi } from "vitest";
import { RoomSocketSayError } from "./roomSocketClient";
import {
  encodedServerFrame,
  event,
  flushPromises,
  handshakeFrames,
  malformedMuteEvent,
  malformedRoleEvent,
  openHarness,
  receiveServerFrame,
  sentClientFrame,
} from "./test/roomSocketHarness";
import { agentSessionFixture } from "./test/agentSession";

function leaveAck(
  requestId: unknown,
  mutate: (result: Record<string, unknown>) => void = () => {}
) {
  const result: Record<string, unknown> = {
    participant: {
      room_id: "general",
      participant_id: "operator-local",
      display_name: "Operator",
      avatar_image_url: "",
      participant_type: "human",
      status: "left",
      role: "human",
      owner_id: "operator-local",
      muted: false,
      created_at: "2026-08-25T00:00:00Z",
      updated_at: "2026-08-25T00:00:01Z",
    },
    event: {
      v: 1,
      id: "leave-event-1",
      created_at: "2026-08-25T00:00:01Z",
      room_id: "general",
      seq: 1,
      type: "participant_left",
      actor: { participant_id: "operator-local", participant_type: "human" },
      participant_id: "operator-local",
      participant_type: "human",
      display_name: "Operator",
    },
    event_seq: 1,
  };
  mutate(result);
  return {
    op: "ack",
    accepted: true,
    resolution: "committed",
    request_id: requestId,
    action: "participant.leave",
    result,
  };
}

function providerAvailability() {
  return {
    id: "codex",
    display_name: "Codex",
    provider_kind: "codex_live_session",
    runtime_kind: "live_cli",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    default_model: "gpt-5.6-luna",
    interactive: true,
    startable: true,
    available: true,
    discovery_status: "ready",
    catalog_source: "discovered",
    credential_available: false,
    controls: [],
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("bounded canonical room socket", () => {
  it("rejects a command before admission when secure request identity is unavailable", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    expect(sockets[0].sent).toHaveLength(1);
    vi.stubGlobal("crypto", {});

    await expect(handle.command("message.send", { content: "blocked" })).rejects.toMatchObject({
      category: "request_id_unavailable",
    });
    expect(sockets[0].sent).toHaveLength(1);
    handle.close();
  });

  it("holds commands and readiness until the exact finite high-water is delivered", async () => {
    const onOpen = vi.fn();
    const onEvents = vi.fn();
    const { handle, sockets } = openHarness({
      onOpen,
      onRoomEvents: onEvents,
    });
    await flushPromises();
    sockets[0].open();
    const pending = handle.command("message.send", { content: "hello" });
    expect(sockets[0].sent).toHaveLength(1);
    expect(handle.ready()).toBe(false);

    const frames = handshakeFrames(1, 2);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await flushPromises();
    expect(onOpen).not.toHaveBeenCalled();
    expect(sockets[0].sent).toHaveLength(1);

    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [event(2)],
      latest_seq: 2,
    });
    await flushPromises();
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(onEvents).toHaveBeenCalledWith([event(2)]);
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    expect(command).toMatchObject({
      op: "command",
      action: "message.send",
      payload: { content: "hello" },
    });
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "message.send",
      result: { event: event(3), event_seq: 3 },
    });
    await expect(pending).resolves.toMatchObject({ accepted: true });
    handle.close();
  });

  it("rejects a snapshot outside the finite receipt cursor", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot.replace('"last_seq":0', '"last_seq":1'));
    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("snapshot_boundary_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("measures the product frame limit in UTF-8 bytes", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    sockets[0].receiveRaw("한".repeat(100_000));
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("frame_too_large"));
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a receipt for a different product surface", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0, (receipt) => {
      receipt.server_surface_digest = "d".repeat(64);
    });
    sockets[0].receive(frames.receipt);
    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("subscription_receipt_scope_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("fails closed on a gap inside authenticated catch-up", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(1, 3);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [event(3)],
      latest_seq: 3,
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_sequence_gap"));
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a malformed role in the snapshot before consuming its cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(1, 1);
    frames.snap.events = [malformedRoleEvent(1)];
    frames.rawSnapshot = JSON.stringify(frames.snap);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);

    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("snapshot_event_invalid"));
    expect(handle.ready()).toBe(false);
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 0 });
    handle.close();
  });

  it("rejects a partial Agent Session in the snapshot", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    (frames.snap as unknown as Record<string, unknown>).agent_sessions = [
      { session_id: "partial-session" },
    ];
    sockets[0].receive(frames.receipt);
    sockets[0].receive(frames.snap);

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("snapshot_agent_session_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects an Agent Session state event outside the generated contract", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [
        {
          ...event(1),
          type: "agent_session_state",
          participant_id: "agent-test",
          agent_session: {
            ...agentSessionFixture({ room_id: "general" }),
            share_activity: true,
          },
        },
      ],
      latest_seq: 1,
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("event_schema_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a malformed role during authenticated catch-up without consuming its cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(1, 2);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [malformedRoleEvent(2)],
      latest_seq: 2,
    });

    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_schema_invalid"));
    expect(handle.ready()).toBe(false);
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 1 });
    handle.close();
  });

  it("rejects a malformed live role event without consuming the last valid cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(1, 1);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [malformedRoleEvent(2)],
      latest_seq: 2,
    });

    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_schema_invalid"));
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 1 });
    handle.close();
  });

  it("rejects a mute event without a canonical target before consuming its cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(1, 1);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [malformedMuteEvent(2)],
      latest_seq: 2,
    });

    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_schema_invalid"));
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 1 });
    handle.close();
  });

  it("resumes from the last verified durable sequence after resync", async () => {
    vi.useFakeTimers();
    const onEvents = vi.fn();
    const onError = vi.fn();
    const { handle, sockets } = openHarness({
      onRoomEvents: onEvents,
      onError,
    });
    await flushPromises();
    sockets[0].open();
    const first = handshakeFrames(0, 0);
    sockets[0].receive(first.receipt);
    sockets[0].receiveRaw(first.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [event(1)],
      latest_seq: 1,
    });
    await vi.waitFor(() => expect(onEvents).toHaveBeenCalledWith([event(1)]));
    const resync = encodedServerFrame({
      op: "resync_required",
      stream: "room_events",
      latest_seq: 1,
      reason: "subscriber lagged",
    });
    const staleEvent = encodedServerFrame({
      op: "event",
      stream: "room_events",
      events: [event(2)],
      latest_seq: 2,
    });
    sockets[0].receiveRaw(resync);
    sockets[0].receiveRaw(staleEvent);
    await vi.waitFor(() => expect(onError).toHaveBeenCalled());
    expect(onEvents).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 1 });
    handle.close();
  });

  it("rejects an ACK whose action differs from the pending command", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    void handle.command("message.send", { content: "hello" }).catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "room.random.roll",
      result: { event: event(1), event_seq: 1 },
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("ack_contract_invalid"));
    handle.close();
  });

  it("rejects a mute ACK without its exact durable participant event", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    void handle
      .command("participant.mute", { participant_id: "agent-one", muted: true })
      .catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "participant.mute",
      result: {
        participant: { participant_id: "agent-one", muted: true },
        event: {
          id: "evt-1",
          seq: 1,
          type: "participant_muted",
          participant_id: "agent-two",
          muted: true,
        },
        event_seq: 1,
      },
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("ack_contract_invalid"));
    handle.close();
  });

  it("fails closed on an authenticated command response for an unknown request", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const pendingLeave = handle.command("participant.leave", {});
    void pendingLeave.catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);

    const unknown = encodedServerFrame({
      op: "nack",
      accepted: false,
      resolution: "rejected",
      request_id: "unknown-request",
      action: "message.send",
      error: {
        code: "message_invalid",
        message: "Message content is invalid.",
      },
    });
    const trailingLeave = encodedServerFrame(leaveAck(command.request_id));
    sockets[0].receiveRaw(unknown);
    sockets[0].receiveRaw(trailingLeave);

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("command_response_unexpected")
    );
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(2);
    handle.close();
  });

  it("settles a command only for a server-declared definitive rejection", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const pending = handle.command("message.send", { content: "rejected" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);

    receiveServerFrame(sockets[0], {
      op: "nack",
      accepted: false,
      resolution: "rejected",
      request_id: command.request_id,
      action: "message.send",
      error: {
        code: "message_invalid",
        message: "Message content is invalid.",
      },
    });

    await expect(pending).rejects.toMatchObject({ category: "message_invalid" });
    expect(sockets[0].readyState).toBe(WebSocket.OPEN);
    handle.close();
  });

  it("does not accept a queued leave ACK after an unresolved outcome", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const pendingLeave = handle.command("participant.leave", {});
    void pendingLeave.catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    const unresolved = encodedServerFrame({
      op: "nack", accepted: false, resolution: "unresolved",
      request_id: command.request_id, action: "participant.leave",
      error: { code: "persistence_failed", message: "Persistence operation failed." },
    });
    const trailingLeave = encodedServerFrame(leaveAck(command.request_id));
    sockets[0].receiveRaw(unresolved);
    sockets[0].receiveRaw(trailingLeave);
    await vi.waitFor(() => expect(sockets[0].readyState).toBe(WebSocket.CLOSED));
    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(2);
    handle.close();
  });

  it("accepts provider catalog pushes only after the subscription is ready", async () => {
    const onProviderCatalog = vi.fn();
    const { handle, sockets } = openHarness({ onProviderCatalog });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await flushPromises();
    receiveServerFrame(sockets[0], {
      op: "provider_catalog_updated",
      catalog: {
        status: "ready",
        catalog_revision: "cat-2",
        providers: [providerAvailability()],
      },
    });
    await vi.waitFor(() =>
      expect(onProviderCatalog).toHaveBeenCalledWith({
        status: "ready",
        catalog_revision: "cat-2",
        providers: [providerAvailability()],
      })
    );
    handle.close();
  });

  it("rejects provider catalog pushes with fields outside the generated contract", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    receiveServerFrame(sockets[0], {
      op: "provider_catalog_updated",
      catalog: {
        status: "ready",
        catalog_revision: "cat-private-field",
        providers: [{ ...providerAvailability(), executable: "/private/bin/codex" }],
      },
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("provider_catalog_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects snapshot provider aliases that disagree with the catalog owner", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    (frames.snap as unknown as Record<string, unknown>).available_providers = [
      providerAvailability(),
    ];
    sockets[0].receive(frames.receipt);
    sockets[0].receive(frames.snap);

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("snapshot_schema_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a snapshot missing a generated room-settings field", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    delete (frames.snap.room_settings as unknown as Record<string, unknown>)
      .activity_plugin;
    sockets[0].receive(frames.receipt);
    sockets[0].receive(frames.snap);

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("snapshot_schema_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("finishes a delivered leave ACK before the server closes", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    const pendingLeave = handle.command("participant.leave", {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);

    receiveServerFrame(sockets[0], leaveAck(command.request_id));
    sockets[0].close();

    await expect(pendingLeave).resolves.toMatchObject({
      accepted: true,
      action: "participant.leave",
    });
    await vi.advanceTimersByTimeAsync(5_000);
    expect(sockets).toHaveLength(1);
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it.each([
    ["missing event sequence", (result: Record<string, unknown>) => { delete result.event_seq; }],
    ["wrong event room", (result: Record<string, unknown>) => {
      (result.event as Record<string, unknown>).room_id = "other";
    }],
    ["wrong event participant", (result: Record<string, unknown>) => {
      (result.event as Record<string, unknown>).participant_id = "other";
    }],
    ["wrong participant room", (result: Record<string, unknown>) => {
      (result.participant as Record<string, unknown>).room_id = "other";
    }],
    ["wrong participant identity", (result: Record<string, unknown>) => {
      (result.participant as Record<string, unknown>).participant_id = "other";
    }],
  ])("rejects a close-before-verify leave ACK with %s", async (_case, mutate) => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = openHarness({
      onError: (error) => { if (error instanceof RoomSocketSayError) errors.push(error); },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const pendingLeave = handle.command("participant.leave", {});
    void pendingLeave.catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    sockets[0].receiveRaw(encodedServerFrame(leaveAck(command.request_id, mutate)));
    sockets[0].close();
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("ack_contract_invalid"));
    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(2);
    handle.close();
  });

});
