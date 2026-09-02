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

function participant(overrides: Record<string, unknown> = {}) {
  return {
    room_id: "general",
    participant_id: "operator-local",
    display_name: "Operator",
    avatar_image_url: "",
    participant_type: "human",
    status: "joined",
    role: "human",
    owner_id: "operator-local",
    muted: false,
    created_at: "2026-08-25T00:00:00Z",
    updated_at: "2026-08-25T00:00:00Z",
    ...overrides,
  };
}

async function expectSnapshotRejected(
  mutate: (snapshot: { participants: unknown[]; agent_sessions: unknown[] }) => void,
) {
  const errors: RoomSocketSayError[] = [];
  const { handle, sockets } = openHarness({
    onError: (error) => {
      if (error instanceof RoomSocketSayError) errors.push(error);
    },
  });
  await flushPromises();
  sockets[0].open();
  const frames = handshakeFrames(0, 0);
  mutate(frames.snap);
  sockets[0].receive(frames.receipt);
  sockets[0].receive(frames.snap);

  await vi.waitFor(() =>
    expect(errors.at(-1)?.category).toMatch(/^snapshot_(participant|agent_session)_invalid$/)
  );
  expect(handle.ready()).toBe(false);
  handle.close();
}

function roleEvent() {
  return {
    ...event(1),
    type: "participant_updated",
    participant_id: "agent-one",
    participant_type: "agent",
    display_name: "Agent One",
    role: "reviewer",
  };
}

function roleAck(requestId: unknown, mutate: (result: Record<string, unknown>) => void = () => {}) {
  const committedEvent = roleEvent();
  const result: Record<string, unknown> = {
    participant: participant({
      participant_id: "agent-one",
      display_name: "Agent One",
      participant_type: "agent",
      role: "reviewer",
      owner_id: "operator-local",
    }),
    event: committedEvent,
    event_seq: committedEvent.seq,
  };
  mutate(result);
  return {
    op: "ack",
    accepted: true,
    resolution: "committed",
    request_id: requestId,
    action: "participant.role.update",
    result,
  };
}

describe("Participant socket contract", () => {
  it.each([
    ["unknown field", (snapshot: { participants: unknown[] }) => {
      snapshot.participants = [{ ...participant(), legacy_status: "online" }];
    }],
    ["duplicate identity", (snapshot: { participants: unknown[] }) => {
      snapshot.participants = [participant(), participant()];
    }],
    ["session bound to a human", (snapshot: { participants: unknown[]; agent_sessions: unknown[] }) => {
      snapshot.participants = [participant({ participant_id: "agent-one" })];
      snapshot.agent_sessions = [agentSessionFixture({
        room_id: "general",
        session_id: "agent-one",
        participant_id: "agent-one",
      })];
    }],
    ["agent without a session", (snapshot: { participants: unknown[] }) => {
      snapshot.participants = [participant({
        participant_id: "agent-one",
        participant_type: "agent",
        role: "agent",
      })];
    }],
  ])("rejects a snapshot with %s", async (_label, mutate) => {
    await expectSnapshotRejected(mutate);
  });

  it("accepts one sequence-bound role update", async () => {
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
    const pending = handle.command("participant.role.update", {
      participant_id: "agent-one",
      role: "reviewer",
    });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    receiveServerFrame(sockets[0], roleAck(command.request_id));

    await expect(pending).resolves.toMatchObject({ accepted: true });
    expect(errors).toEqual([]);
    handle.close();
  });

  it.each([
    ["missing event sequence", (result: Record<string, unknown>) => {
      delete result.event_seq;
    }],
    ["unknown participant field", (result: Record<string, unknown>) => {
      (result.participant as Record<string, unknown>).connection_kind = "legacy";
    }],
  ])("rejects a role update with %s", async (_label, mutate) => {
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
    void handle.command("participant.role.update", {
      participant_id: "agent-one",
      role: "reviewer",
    }).catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    receiveServerFrame(sockets[0], roleAck(command.request_id, mutate));

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("ack_contract_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });
});
