import { describe, expect, it, vi } from "vitest";
import { RoomSocketSayError } from "./roomSocketClient";
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
    status: "detached",
    role: "agent",
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

describe("Agent Session socket contract", () => {
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
});
