import { afterEach, describe, expect, it, vi } from "vitest";
import { openRoomSocket, RoomSocketSayError } from "./roomSocketClient";
import {
  deriveConnectionNonce,
  digestPermissions,
  digestSnapshotFrame,
  subscriptionProofTranscript,
} from "./lib/serverProof";
import { utf8 } from "./lib/lengthDelimitedCrypto";
import type { SubscriptionReceipt } from "./lib/roomSubscriptionContract";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";

const PROOF_KEY = "b".repeat(64);
const CAPABILITIES = {
  "agent.control": true,
  "bridge.publish": false,
  "bridge.report": false,
  "message.modify": true,
  "message.send": true,
  "participant.kick": true,
  "participant.leave": true,
  "participant.mute": true,
  "provider.request.resolve": true,
  "room.delete": true,
  "room.history": true,
  "room.manage": true,
  "room.random": true,
  "room.vote.summary": true,
};

class FakeWebSocket {
  readyState: number = WebSocket.CONNECTING;
  sent: Array<Record<string, unknown>> = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;

  send(raw: string) {
    this.sent.push(JSON.parse(raw) as Record<string, unknown>);
  }

  open() {
    this.readyState = WebSocket.OPEN;
    this.onopen?.(new Event("open"));
  }

  receive(message: Record<string, unknown>) {
    this.receiveRaw(JSON.stringify(message));
  }

  receiveRaw(message: string) {
    if (this.readyState === WebSocket.CLOSED) return;
    this.onmessage?.({ data: message } as MessageEvent);
  }

  close() {
    if (this.readyState === WebSocket.CLOSED) return;
    this.readyState = WebSocket.CLOSED;
    this.onclose?.({} as CloseEvent);
  }
}

function event(seq: number) {
  return {
    v: 1,
    id: `evt-${seq}`,
    room_id: "general",
    seq,
    type: "message_final",
    content: `message ${seq}`,
  };
}

function snapshot(cursor: number) {
  const events = Array.from({ length: cursor }, (_, index) => event(index + 1));
  return {
    op: "snapshot",
    stream: "room_events",
    room: { room_id: "general" },
    room_settings: {},
    participants: [],
    agent_sessions: [],
    active_turns: [],
    events,
    oldest_seq: events[0]?.seq || 0,
    last_seq: cursor,
    has_more_before: false,
    resume_gap: false,
    snapshot_mode: "initial",
    provider_catalog: { status: "ready", catalog_revision: "cat-1", providers: [] },
    available_providers: [],
    capabilities: CAPABILITIES,
  };
}

async function signReceipt(receipt: SubscriptionReceipt): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    utf8(PROOF_KEY),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const signature = new Uint8Array(
    await crypto.subtle.sign("HMAC", key, subscriptionProofTranscript(receipt))
  );
  return Array.from(signature, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function handshakeFrames(
  socket: FakeWebSocket,
  ticket: string,
  snapshotCursor: number,
  catchupHighWater: number,
  mutateReceipt?: (receipt: SubscriptionReceipt) => void
) {
  const subscription = socket.sent[0];
  const snap = snapshot(snapshotCursor);
  const rawSnapshot = JSON.stringify(snap);
  const receipt: SubscriptionReceipt = {
    op: "subscribed",
    streams: ["room_events"],
    protocol_version: 1,
    server_challenge: String(subscription.server_challenge),
    connection_nonce: await deriveConnectionNonce(ticket),
    room_id: "general",
    principal_id: "operator",
    participant_id: "operator-local",
    server_surface_revision: TEST_SERVER_PRODUCT_SURFACE.revision,
    server_surface_digest: TEST_SERVER_PRODUCT_SURFACE.digest,
    permissions_digest: String(await digestPermissions(CAPABILITIES)),
    snapshot_cursor: snapshotCursor,
    catchup_high_water: catchupHighWater,
    snapshot_digest: await digestSnapshotFrame(rawSnapshot),
    proof: "",
  };
  mutateReceipt?.(receipt);
  receipt.proof = await signReceipt(receipt);
  return { receipt, snap, rawSnapshot };
}

function openHarness(handlers: Parameters<typeof openRoomSocket>[2] = {}) {
  const sockets: FakeWebSocket[] = [];
  let issued = 0;
  const tickets: string[] = [];
  const handle = openRoomSocket(
    { kind: "host", meetingId: "general" },
    ["room_events"],
    handlers,
    {
      getTicket: async () => {
        issued += 1;
        const ticket = issued.toString(16).padStart(64, "a");
        tickets.push(ticket);
        return {
          ticket,
          websocket_base_url: "ws://127.0.0.1:43123",
          server_proof_key: PROOF_KEY,
        };
      },
      createSocket: () => {
        const socket = new FakeWebSocket();
        sockets.push(socket);
        return socket as unknown as WebSocket;
      },
      serverSurface: TEST_SERVER_PRODUCT_SURFACE,
      expectedRoomId: "general",
      expectedParticipantId: "operator-local",
    }
  );
  return { handle, sockets, tickets };
}

async function flushPromises() {
  if (vi.isFakeTimers()) {
    await vi.advanceTimersByTimeAsync(0);
  } else {
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  }
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

afterEach(() => {
  vi.useRealTimers();
});

describe("proof-bound canonical room socket", () => {
  it("holds commands and readiness until the exact finite high-water is delivered", async () => {
    const onOpen = vi.fn();
    const onEvents = vi.fn();
    const { handle, sockets, tickets } = openHarness({
      onOpen,
      onRoomEvents: onEvents,
    });
    await flushPromises();
    sockets[0].open();
    const pending = handle.command("message.send", { content: "hello" });
    expect(sockets[0].sent).toHaveLength(1);
    expect(handle.ready()).toBe(false);

    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 2);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await flushPromises();
    expect(onOpen).not.toHaveBeenCalled();
    expect(sockets[0].sent).toHaveLength(1);

    sockets[0].receive({
      op: "event",
      stream: "room_events",
      events: [event(2)],
      latest_seq: 2,
    });
    await flushPromises();
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(onEvents).toHaveBeenCalledWith([event(2)]);
    const command = sockets[0].sent[1];
    expect(command).toMatchObject({
      op: "command",
      action: "message.send",
      payload: { content: "hello" },
    });
    sockets[0].receive({
      op: "ack",
      accepted: true,
      request_id: command.request_id,
      action: "message.send",
      result: { event: event(3), event_seq: 3 },
    });
    await expect(pending).resolves.toMatchObject({ accepted: true });
    handle.close();
  });

  it("rejects snapshot bytes that differ from the signed digest", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot.replace('"room_settings":{}', '"room_settings":{"label":"tampered"}'));
    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("snapshot_binding_invalid")
    );
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a validly signed receipt for a different product surface", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0, (receipt) => {
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
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 3);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    sockets[0].receive({
      op: "event",
      stream: "room_events",
      events: [event(3)],
      latest_seq: 3,
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_sequence_gap"));
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("resumes from the last verified durable sequence after resync", async () => {
    vi.useFakeTimers();
    const onEvents = vi.fn();
    const onError = vi.fn();
    const { handle, sockets, tickets } = openHarness({
      onRoomEvents: onEvents,
      onError,
    });
    await flushPromises();
    sockets[0].open();
    const first = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(first.receipt);
    sockets[0].receiveRaw(first.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    sockets[0].receive({
      op: "event",
      stream: "room_events",
      events: [event(1)],
      latest_seq: 1,
    });
    await vi.waitFor(() => expect(onEvents).toHaveBeenCalledWith([event(1)]));
    sockets[0].receive({
      op: "resync_required",
      stream: "room_events",
      latest_seq: 1,
      reason: "subscriber lagged",
    });
    await vi.waitFor(() => expect(onError).toHaveBeenCalled());
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets).toHaveLength(2));
    sockets[1].open();
    expect(sockets[1].sent[0]).toMatchObject({ resume_from_seq: 1 });
    handle.close();
  });

  it("rejects an ACK whose action differs from the pending command", async () => {
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    void handle.command("message.send", { content: "hello" }).catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sockets[0].sent[1];
    sockets[0].receive({
      op: "ack",
      accepted: true,
      request_id: command.request_id,
      action: "room.random.roll",
      result: { event: event(1), event_seq: 1 },
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("ack_contract_invalid"));
    handle.close();
  });

  it("accepts provider catalog pushes only after the subscription is ready", async () => {
    const onProviderCatalog = vi.fn();
    const { handle, sockets, tickets } = openHarness({ onProviderCatalog });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await flushPromises();
    sockets[0].receive({
      op: "provider_catalog_updated",
      catalog: { status: "ready", catalog_revision: "cat-2", providers: [] },
    });
    await vi.waitFor(() =>
      expect(onProviderCatalog).toHaveBeenCalledWith({
        status: "ready",
        catalog_revision: "cat-2",
        providers: [],
      })
    );
    handle.close();
  });
});
