import { afterEach, describe, expect, it, vi } from "vitest";
import { openRoomSocket, RoomSocketSayError } from "./roomSocketClient";
import { FakeWebSocket, flushPromises } from "./roomSocketTestSupport";
import { agentSessionIsValid } from "./roomSocketSchema";

afterEach(() => {
  vi.useRealTimers();
});

describe("Agent Session socket receipts", () => {
  it("accepts every public runtime lifecycle state without private authority", () => {
    const session = {
      room_id: "general",
      session_id: "codex-session-1",
      participant_id: "codex-session-1",
      display_name: "Terra",
      runtime_status: "stopped",
      process_ownership: "server",
      external_owned: false,
      provider_kind: "codex_live_session",
      model: "gpt-5.6-terra",
    };
    for (const runtime_status of [
      "stopped", "available", "starting", "idle", "busy", "paused",
      "recovering", "stopping", "error", "disconnected",
    ]) {
      expect(agentSessionIsValid({ ...session, runtime_status }, "general")).toBe(true);
    }
    expect(agentSessionIsValid({ ...session, runtime_status: "unknown" }, "general")).toBe(false);
    expect(agentSessionIsValid({ ...session, runtime_status: "idle", runtime_handle_id: "private" }, "general")).toBe(false);
    expect(agentSessionIsValid({ ...session, runtime_status: "idle", runtime_owner_id: "private" }, "general")).toBe(false);
  });

  it("accepts agent.create only with its durable stopped-session receipt", async () => {
    vi.useFakeTimers();
    const sockets: FakeWebSocket[] = [];
    const errors: RoomSocketSayError[] = [];
    const handle = openRoomSocket(
      { kind: "host", meetingId: "general" },
      ["room_events"],
      { onError: (error) => { if (error instanceof RoomSocketSayError) errors.push(error); } },
      {
        getTicket: async () => `ticket-${sockets.length + 1}`,
        createSocket: () => {
          const socket = new FakeWebSocket();
          sockets.push(socket);
          return socket as unknown as WebSocket;
        },
      }
    );
    await flushPromises();
    sockets[0].open();
    const pending = handle.command("agent.create", { provider_id: "codex" });
    const command = sockets[0].sent[1];
    sockets[0].receive({
      op: "ack",
      accepted: true,
      request_id: command.request_id,
      action: "agent.create",
      result: {
        event_seq: 1,
        event: {
          id: "evt-agent-1",
          room_id: "general",
          seq: 1,
          type: "agent_session_created",
          participant_id: "codex-session-1",
        },
        agent_session: {
          room_id: "general",
          session_id: "codex-session-1",
          participant_id: "codex-session-1",
          display_name: "Terra",
          runtime_status: "stopped",
          process_ownership: "server",
          external_owned: false,
          provider_kind: "codex_live_session",
          model: "gpt-5.6-terra",
        },
      },
    });
    await expect(pending).resolves.toMatchObject({ action: "agent.create" });

    const invalid = handle.command("agent.create", { provider_id: "codex" });
    const invalidCommand = sockets[0].sent.at(-1) as Record<string, unknown>;
    sockets[0].receive({
      op: "ack",
      accepted: true,
      request_id: invalidCommand.request_id,
      action: "agent.create",
      result: { agent_session: { session_id: "uncommitted" } },
    });
    await flushPromises();
    expect(errors.at(-1)?.category).toBe("ack_result_invalid");
    handle.close();
    await expect(invalid).rejects.toMatchObject({ category: "socket_closed" });
  });
});
