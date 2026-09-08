import { describe, expect, it, vi } from "vitest";
import { RoomSocketSayError } from "./roomSocketClient";
import { commandAckResultIsValid, publicRoomEventIsValid, snapshotValidationError } from "./lib/roomSocketValidation";
import { applyParticipantEvents, agentSessionUpdatesFromEvents } from "./lib/canonicalRoomProjection";
import type { RoomEvent } from "./api";
import { agentSessionFixture } from "./test/agentSession";
import {
  event,
  flushPromises,
  handshakeFrames,
  openHarness,
  receiveServerFrame,
  sentClientFrame,
} from "./test/roomSocketHarness";

async function openReadyHarness(errors: RoomSocketSayError[]) {
  const harness = openHarness({
    onError: (error) => {
      if (error instanceof RoomSocketSayError) errors.push(error);
    },
  });
  await flushPromises();
  harness.sockets[0].open();
  const frames = handshakeFrames(0, 0);
  harness.sockets[0].receive(frames.receipt);
  harness.sockets[0].receiveRaw(frames.rawSnapshot);
  await vi.waitFor(() => expect(harness.handle.ready()).toBe(true));
  return harness;
}

function creationRecords() {
  const participant = {
    room_id: "general",
    participant_id: "agent-created",
    display_name: "Created Agent",
    avatar_image_url: "",
    participant_type: "agent",
    status: "detached" as const,
    role: "agent" as const,
    owner_id: "operator-local",
    muted: false,
    created_at: "2026-08-25T00:00:01Z",
    updated_at: "2026-08-25T00:00:01Z",
  };
  const session = agentSessionFixture({
    room_id: "general",
    session_id: "agent-created",
    participant_id: "agent-created",
    display_name: "Created Agent",
    status: "available",
    runtime_status: "stopped",
    enabled: false,
  });
  const createdEvent = {
    ...event(1),
    type: "agent_session_created",
    participant_id: "agent-created",
    participant_type: "agent",
    session_id: "agent-created",
    provider_kind: session.provider_kind,
    display_name: "Created Agent",
    participant,
    agent_session: session,
  };
  return { participant, session, createdEvent };
}

function creationStartRecords() {
  const { participant, session } = creationRecords();
  const preparedSession = {
    ...session,
    runtime_status: "starting",
    enabled: true,
    updated_at: "2026-08-25T00:00:02Z",
  };
  const createdEvent = {
    ...event(1),
    type: "agent_session_created",
    participant_id: "agent-created",
    participant_type: "agent",
    session_id: "agent-created",
    provider_kind: session.provider_kind,
    display_name: "Created Agent",
    participant,
    agent_session: preparedSession,
  };
  const joinedParticipant = {
    ...participant,
    status: "joined",
    updated_at: "2026-08-25T00:00:03Z",
  };
  const joinedEvent = {
    ...event(2),
    type: "participant_joined",
    participant_id: "agent-created",
    participant_type: "agent",
    display_name: "Created Agent",
    participant: joinedParticipant,
  };
  const attachedEvent = {
    ...event(3),
    type: "session_attached",
    participant_id: "agent-created",
    participant_type: "agent",
    display_name: "Created Agent",
  };
  const finalSession = {
    ...preparedSession,
    status: "attached",
    runtime_status: "idle",
    provider_session_active: true,
    updated_at: "2026-08-25T00:00:04Z",
  };
  const stateEvent = {
    ...event(4),
    type: "agent_session_state",
    participant_id: "agent-created",
    participant_type: "agent",
    session_id: "agent-created",
    runtime_status: "idle",
    display_name: "Created Agent",
    agent_session: finalSession,
  };
  const events = [createdEvent, joinedEvent, attachedEvent, stateEvent];
  return {
    participant,
    initialSession: session,
    events,
    start: {
      agent_session: finalSession,
      runtime_reused: false,
      events: events.slice(1),
      event: stateEvent,
    },
  };
}

describe("Agent Session socket contract", () => {
  it("projects an admitted external attendee while rejecting mixed custody and managed creation ACKs", async () => {
    const { participant, session, createdEvent } = creationRecords();
    const joined = { ...participant, status: "joined" as const };
    const external = { ...session, status: "attached", runtime_status: "disconnected",
      enabled: false, external_owned: true, process_ownership: "external",
      runtime_kind: "external_attendee", connection_kind: "canonical_room_websocket", transport: "websocket" };
    const admitted = { ...createdEvent, participant: joined, agent_session: external } as RoomEvent;
    expect(publicRoomEventIsValid(admitted, "general")).toBe(true);
    expect(applyParticipantEvents([], [admitted])).toEqual([joined]);
    expect(agentSessionUpdatesFromEvents([admitted])).toEqual([external]);
    const frames = handshakeFrames(1, 1);
    const snapshot = JSON.parse(frames.rawSnapshot);
    Object.assign(snapshot, { participants: [joined], agent_sessions: [external], events: [admitted] });
    expect(snapshotValidationError(snapshot, { expectedRoomId: "general", currentLastSeq: 0 })).toBeNull();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    receiveServerFrame(sockets[0], { op: "event", stream: "room_events", events: [admitted], latest_seq: 1 });
    expect(handle.ready()).toBe(true);
    expect(errors).toEqual([]);
    handle.close();
    for (const conflict of [{ process_ownership: "server" }, { runtime_kind: "native_cli" },
      { status: "available" }, { runtime_status: "idle" }, { enabled: true },
      { provider_session_active: true }, { connection_kind: "native_cli_bridge" }]) {
      expect(publicRoomEventIsValid({ ...admitted, agent_session: { ...external, ...conflict } }, "general")).toBe(false);
    }
    expect(publicRoomEventIsValid({ ...admitted, participant }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...admitted, type: "agent_session_reactivated" }, "general")).toBe(false);
    expect(commandAckResultIsValid("agent.create", {}, { status: "created", participant: joined,
      agent_session: external, event: admitted, events: [admitted] }, "general", "operator-local")).toBe(false);
  });

  it("projects re-added membership without overwriting room role, mute or name and rejects mismatched ACKs", () => {
    const { participant, session, createdEvent } = creationRecords();
    const restored = { ...participant, display_name: "Room name", role: "director", muted: true };
    const reactivated = { ...createdEvent, type: "agent_session_reactivated", participant: restored };
    const result = { status: "readded", participant: restored, agent_session: session,
      events: [reactivated], event: reactivated, event_seq: reactivated.seq };
    const valid = (candidate: unknown) => commandAckResultIsValid("agent.readd",
      { agent_id: session.session_id, start: false }, candidate, "general", "operator-local");
    expect(valid(result)).toBe(true);
    expect(valid({ ...result, event_seq: undefined })).toBe(false);
    expect(valid({ ...result, participant: { ...restored, muted: false } })).toBe(false);
    expect(valid({ ...result, agent_session: { ...session, enabled: true } })).toBe(false);
    expect(applyParticipantEvents([], [reactivated as unknown as RoomEvent])).toEqual([restored]);
    expect(agentSessionUpdatesFromEvents([reactivated as unknown as RoomEvent])).toEqual([session]);
  });

  it("accepts only the joined and attached final projection for re-add with start", () => {
    const { start } = creationStartRecords();
    const participant = (start.events[0] as unknown as { participant: unknown }).participant;
    const result = { ...start, status: "readded", participant, event_seq: start.event.seq };
    const valid = (candidate: unknown) => commandAckResultIsValid("agent.readd",
      { agent_id: "agent-created", start: true }, candidate, "general", "operator-local");
    expect(valid(result)).toBe(true);
    expect(valid({ ...result, events: start.events.slice(1) })).toBe(false);
    expect(valid({ ...result, agent_session: { ...start.agent_session, runtime_status: "stopped" } })).toBe(false);
  });

  it.each(["agent_id", "participant_id", "session_id"])("binds listing and started re-add ACKs to the requested %s", (key) => {
    const { participant, session, createdEvent } = creationRecords();
    const event = { ...createdEvent, type: "agent_session_reactivated" };
    const listing = { status: "readded", participant, agent_session: session,
      events: [event], event, event_seq: event.seq };
    const { start } = creationStartRecords();
    const started = { ...start, status: "readded", event_seq: start.event.seq,
      participant: (start.events[0] as unknown as { participant: unknown }).participant };
    for (const [startRequested, result] of [[false, listing], [true, started]] as const) {
      const valid = (payload: Record<string, unknown>) => commandAckResultIsValid("agent.readd",
        { ...payload, start: startRequested }, result, "general", "operator-local");
      expect(valid({ [key]: session.session_id })).toBe(true);
      expect(valid({ [key]: "another-session" })).toBe(false);
      expect(valid({})).toBe(false);
      expect(valid({ [key]: "" })).toBe(false);
      expect(valid({ agent_id: session.session_id, session_id: session.session_id })).toBe(false);
    }
  });

  it.each([false, true])("resolves complete re-add socket ACKs including deduplicated responses with start=%s", async (startRequested) => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    const { participant, session, createdEvent } = creationRecords();
    const reactivated = { ...createdEvent, type: "agent_session_reactivated" };
    const { start } = creationStartRecords();
    const result = startRequested
      ? { ...start, status: "readded", event_seq: start.event.seq,
          participant: (start.events[0] as unknown as { participant: unknown }).participant }
      : { status: "readded", participant, agent_session: session,
          events: [reactivated], event: reactivated, event_seq: reactivated.seq };
    for (const deduplicated of [false, true]) {
      const sentCount = sockets[0].sent.length;
      const pending = handle.command("agent.readd", { agent_id: session.session_id, start: startRequested });
      await vi.waitFor(() => expect(sockets[0].sent.length).toBe(sentCount + 1));
      const command = sentClientFrame(sockets[0], sentCount);
      receiveServerFrame(sockets[0], { op: "ack", accepted: true, resolution: "committed",
        request_id: command.request_id, action: "agent.readd", deduplicated, result });
      await expect(pending).resolves.toMatchObject({ result });
      expect(handle.ready()).toBe(true);
    }
    expect(errors).toEqual([]);
    handle.close();
  });

  it("rejects a state event without its participant binding", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);

    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [{
        ...event(1),
        type: "agent_session_state",
        agent_session: agentSessionFixture({
          room_id: "general",
          participant_id: "agent-test",
        }),
      }],
      latest_seq: 1,
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("event_schema_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it.each([
    ["session identity", { session_id: "other-session" }],
    ["runtime status", { runtime_status: "busy" }],
    ["display identity", { display_name: "Conflicting Agent" }],
    ["participant kind", { participant_type: "human" }],
  ])("rejects a state event with conflicting %s", async (_label, conflict) => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    const session = agentSessionFixture({
      room_id: "general",
      session_id: "agent-test",
      participant_id: "agent-test",
      display_name: "Agent Test",
      runtime_status: "idle",
    });

    receiveServerFrame(sockets[0], {
      op: "event",
      stream: "room_events",
      events: [{
        ...event(1),
        type: "agent_session_state",
        participant_id: session.participant_id,
        participant_type: "agent",
        session_id: session.session_id,
        runtime_status: session.runtime_status,
        display_name: session.display_name,
        agent_session: session,
        ...conflict,
      }],
      latest_seq: 1,
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("event_schema_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a create ACK whose top-level session conflicts with its event", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    void handle.command("agent.create", { provider_id: "codex" }).catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    const { participant, session, createdEvent } = creationRecords();
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "agent.create",
      result: {
        status: "created",
        participant,
        agent_session: { ...session, participant_id: "agent-other" },
        event: createdEvent,
        event_seq: 1,
        events: [createdEvent],
      },
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("ack_contract_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("accepts one coherent committed create ACK", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    const pending = handle.command("agent.create", { provider_id: "codex" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    const { participant, session, createdEvent } = creationRecords();
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "agent.create",
      result: {
        status: "created",
        participant,
        agent_session: session,
        event: createdEvent,
        event_seq: 1,
        events: [createdEvent],
      },
    });

    await expect(pending).resolves.toMatchObject({ accepted: true });
    expect(errors).toEqual([]);
    handle.close();
  });

  it.each([false, true])(
    "accepts the committed create-and-start ACK shape (deduplicated=%s)",
    async (deduplicated) => {
      const errors: RoomSocketSayError[] = [];
      const { handle, sockets } = await openReadyHarness(errors);
      const pending = handle.command("agent.create", {
        provider_id: "codex",
        start: true,
      });
      await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
      const command = sentClientFrame(sockets[0]);
      const { participant, initialSession, events, start } = creationStartRecords();
      receiveServerFrame(sockets[0], {
        op: "ack",
        accepted: true,
        resolution: "committed",
        deduplicated,
        request_id: command.request_id,
        action: "agent.create",
        result: {
          status: "created",
          participant,
          agent_session: initialSession,
          start,
          events,
          event: events.at(-1),
          event_seq: 4,
        },
      });

      await expect(pending).resolves.toMatchObject({ accepted: true, deduplicated });
      expect(errors).toEqual([]);
      handle.close();
    },
  );

  it.each(["nested event", "final event"] as const)(
    "rejects a create-and-start ACK with a conflicting %s projection",
    async (conflict) => {
      const errors: RoomSocketSayError[] = [];
      const { handle, sockets } = await openReadyHarness(errors);
      void handle.command("agent.create", {
        provider_id: "codex",
        start: true,
      }).catch(() => {});
      await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
      const command = sentClientFrame(sockets[0]);
      const { participant, initialSession, events, start } = creationStartRecords();
      const conflictingStart = conflict === "nested event"
        ? {
            ...start,
            events: start.events.map((event, index) =>
              index === 1 ? { ...event, display_name: "Conflicting Agent" } : event
            ),
          }
        : start;
      const finalEvent = conflict === "final event"
        ? {
            ...events[3],
            actor: {
              participant_id: "conflicting-actor",
              participant_type: "human",
            },
          }
        : events[3];
      receiveServerFrame(sockets[0], {
        op: "ack",
        accepted: true,
        resolution: "committed",
        request_id: command.request_id,
        action: "agent.create",
        result: {
          status: "created",
          participant,
          agent_session: initialSession,
          start: conflictingStart,
          events,
          event: finalEvent,
          event_seq: 4,
        },
      });

      await vi.waitFor(() =>
        expect(errors.at(-1)?.category).toBe("ack_contract_invalid")
      );
      expect(handle.ready()).toBe(false);
      handle.close();
    },
  );

  it.each([
    ["wrong final identity", { status: "available", model: "conflicting-model" }],
    ["inactive provider session", { provider_session_active: false }],
  ])("rejects a create-and-start ACK with %s", async (_label, conflict) => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets } = await openReadyHarness(errors);
    void handle.command("agent.create", {
      provider_id: "codex",
      start: true,
    }).catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    const { participant, initialSession, events, start } = creationStartRecords();
    const invalidSession = {
      ...(events[3] as Record<string, unknown>).agent_session as Record<string, unknown>,
      ...conflict,
    };
    const invalidFinal = { ...events[3], agent_session: invalidSession };
    const invalidEvents = [...events.slice(0, 3), invalidFinal];
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "agent.create",
      result: {
        status: "created",
        participant,
        agent_session: initialSession,
        start: {
          ...start,
          agent_session: invalidSession,
          events: invalidEvents.slice(1),
          event: invalidFinal,
        },
        events: invalidEvents,
        event: invalidFinal,
        event_seq: 4,
      },
    });

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("ack_contract_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });
});

it("binds profile ACK identity and projects both committed events", () => {
  const { participant, session, createdEvent } = creationRecords();
  const updated = { ...session, display_name: "Renamed", updated_at: createdEvent.created_at };
  const member = { ...participant, display_name: "Renamed", updated_at: updated.updated_at };
  const memberEvent = { ...createdEvent, type: "participant_updated", display_name: "Renamed", avatar_image_url: updated.avatar_image_url };
  const stateEvent = { ...createdEvent, ...event(2), type: "agent_session_state",
    runtime_status: updated.runtime_status, display_name: "Renamed", agent_session: updated };
  const result = { participant: member, agent_session: updated,
    events: [memberEvent, stateEvent], event: stateEvent, event_seq: 2 };
  const valid = (value: unknown) => commandAckResultIsValid("agent.profile.update",
    { agent_id: session.participant_id, display_name: "Renamed" }, value, "general", "operator-local");
  expect(valid(result)).toBe(true);
  expect(valid({ ...result, event_seq: undefined })).toBe(false);
  expect(valid({ ...result, participant: { ...member, updated_at: "different" } })).toBe(false);
  expect(valid({ ...result, events: [{ ...memberEvent, created_at: "different" }, stateEvent] })).toBe(false);
  expect(valid({ ...result, participant: { ...member, participant_id: "foreign" } })).toBe(false);
  expect(valid({ ...result, event: { ...stateEvent, agent_session: session } })).toBe(false);
  expect(applyParticipantEvents([participant], result.events as unknown as RoomEvent[])[0].display_name).toBe("Renamed");
  expect(agentSessionUpdatesFromEvents(result.events as unknown as RoomEvent[])).toEqual([updated]);
});
