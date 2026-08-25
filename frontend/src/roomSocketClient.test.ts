import { afterEach, describe, expect, it, vi } from "vitest";
import { openRoomSocket, RoomSocketSayError } from "./roomSocketClient";
import {
  deriveConnectionNonce,
  digestPermissions,
  digestSnapshotFrame,
  subscriptionProofTranscript,
} from "./lib/serverProof";
import { utf8 } from "./lib/lengthDelimitedCrypto";
import {
  decodeAuthenticatedFrame,
  deriveAuthenticatedFrameKey,
  encodeAuthenticatedFrame,
} from "./lib/authenticatedFrames";
import type { SubscriptionReceipt } from "./lib/roomSubscriptionContract";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";

const PROOF_KEY = "b".repeat(64);
const ROOM_SOCKET_COMMAND_TIMEOUT_MS_FOR_TEST = 20_000;
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

function malformedRoleEvent(seq: number) {
  return {
    v: 1,
    id: `evt-${seq}`,
    room_id: "general",
    seq,
    type: "participant_updated",
    participant_id: "agent-one",
    role: "host",
  };
}

function malformedMuteEvent(seq: number) {
  return {
    v: 1,
    id: `evt-${seq}`,
    room_id: "general",
    seq,
    type: "participant_muted",
    participant_id: "",
    muted: true,
  };
}

function snapshot(cursor: number) {
  const events: Array<ReturnType<typeof event> | ReturnType<typeof malformedRoleEvent>> =
    Array.from({ length: cursor }, (_, index) => event(index + 1));
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
  return {
    receipt,
    snap,
    rawSnapshot,
    frameKey: await deriveAuthenticatedFrameKey(PROOF_KEY, receipt.connection_nonce),
    connectionNonce: receipt.connection_nonce,
    serverCounter: 0,
  };
}

async function receiveAuthenticated(
  socket: FakeWebSocket,
  frames: Awaited<ReturnType<typeof handshakeFrames>>,
  message: Record<string, unknown>
) {
  frames.serverCounter += 1;
  socket.receiveRaw(await encodeAuthenticatedFrame(
    frames.frameKey,
    frames.connectionNonce,
    "server",
    frames.serverCounter,
    JSON.stringify(message)
  ));
}

async function sentAuthenticatedCommand(
  socket: FakeWebSocket,
  frames: Awaited<ReturnType<typeof handshakeFrames>>,
  index = 1,
  counter = 1
) {
  const raw = JSON.stringify(socket.sent[index]);
  const payload = await decodeAuthenticatedFrame(
    frames.frameKey,
    frames.connectionNonce,
    "client",
    counter,
    raw
  );
  return JSON.parse(payload) as Record<string, unknown>;
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
  vi.restoreAllMocks();
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

    await receiveAuthenticated(sockets[0], frames, {
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
    const command = await sentAuthenticatedCommand(sockets[0], frames);
    expect(command).toMatchObject({
      op: "command",
      action: "message.send",
      payload: { content: "hello" },
    });
    await receiveAuthenticated(sockets[0], frames, {
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
    await receiveAuthenticated(sockets[0], frames, {
      op: "event",
      stream: "room_events",
      events: [event(3)],
      latest_seq: 3,
    });
    await vi.waitFor(() => expect(errors.at(-1)?.category).toBe("event_sequence_gap"));
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("rejects a malformed role in the signed snapshot before consuming its cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 1);
    frames.snap.events = [malformedRoleEvent(1)];
    frames.rawSnapshot = JSON.stringify(frames.snap);
    frames.receipt.snapshot_digest = await digestSnapshotFrame(frames.rawSnapshot);
    frames.receipt.proof = await signReceipt(frames.receipt);
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

  it("rejects a malformed role during authenticated catch-up without consuming its cursor", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 2);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await receiveAuthenticated(sockets[0], frames, {
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
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 1);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await receiveAuthenticated(sockets[0], frames, {
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
    const { handle, sockets, tickets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 1, 1);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await receiveAuthenticated(sockets[0], frames, {
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
    await receiveAuthenticated(sockets[0], first, {
      op: "event",
      stream: "room_events",
      events: [event(1)],
      latest_seq: 1,
    });
    await vi.waitFor(() => expect(onEvents).toHaveBeenCalledWith([event(1)]));
    await receiveAuthenticated(sockets[0], first, {
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
    const command = await sentAuthenticatedCommand(sockets[0], frames);
    await receiveAuthenticated(sockets[0], frames, {
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
    void handle
      .command("participant.mute", { participant_id: "agent-one", muted: true })
      .catch(() => {});
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = await sentAuthenticatedCommand(sockets[0], frames);
    await receiveAuthenticated(sockets[0], frames, {
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

    await receiveAuthenticated(sockets[0], frames, {
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

    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("command_response_unexpected")
    );
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    handle.close();
  });

  it("settles a command only for a server-declared definitive rejection", async () => {
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const pending = handle.command("message.send", { content: "rejected" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = await sentAuthenticatedCommand(sockets[0], frames);

    await receiveAuthenticated(sockets[0], frames, {
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

  it("accepts provider catalog pushes only after the subscription is ready", async () => {
    const onProviderCatalog = vi.fn();
    const { handle, sockets, tickets } = openHarness({ onProviderCatalog });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await flushPromises();
    await receiveAuthenticated(sockets[0], frames, {
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

  it("replays the exact request after authenticated ACK silence", async () => {
    vi.useFakeTimers();
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const firstFrames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(firstFrames.receipt);
    sockets[0].receiveRaw(firstFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    const commandPayload = { content: "commit once" };
    const pendingCommand = handle.command("message.send", commandPayload);
    let settled = false;
    void pendingCommand.then(
      () => { settled = true; },
      () => { settled = true; }
    );
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const firstCommand = await sentAuthenticatedCommand(sockets[0], firstFrames);
    commandPayload.content = "mutated after send";

    await vi.advanceTimersByTimeAsync(ROOM_SOCKET_COMMAND_TIMEOUT_MS_FOR_TEST);
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(2);
    sockets[1].open();
    const secondFrames = await handshakeFrames(sockets[1], tickets[1], 0, 0);
    sockets[1].receive(secondFrames.receipt);
    sockets[1].receiveRaw(secondFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[1].sent).toHaveLength(2));
    const replayedCommand = await sentAuthenticatedCommand(sockets[1], secondFrames);
    expect(replayedCommand).toEqual(firstCommand);

    await receiveAuthenticated(sockets[1], secondFrames, {
      op: "nack",
      accepted: false,
      resolution: "unresolved",
      request_id: replayedCommand.request_id,
      action: "message.send",
      error: {
        code: "persistence_failed",
        message: "Persistence operation failed.",
      },
    });
    await vi.waitFor(() => expect(sockets[1].readyState).toBe(WebSocket.CLOSED));
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(3);
    sockets[2].open();
    const thirdFrames = await handshakeFrames(sockets[2], tickets[2], 0, 0);
    sockets[2].receive(thirdFrames.receipt);
    sockets[2].receiveRaw(thirdFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[2].sent).toHaveLength(2));
    const replayedAfterUnresolved = await sentAuthenticatedCommand(sockets[2], thirdFrames);
    expect(replayedAfterUnresolved).toEqual(firstCommand);

    await receiveAuthenticated(sockets[2], thirdFrames, {
      op: "nack",
      accepted: false,
      resolution: "unresolved",
      request_id: replayedAfterUnresolved.request_id,
      action: "message.send",
      error: {
        code: "runtime_effect_unconfirmed",
        message: "Provider effect remains unresolved.",
      },
    });
    await vi.waitFor(() => expect(sockets[2].readyState).toBe(WebSocket.CLOSED));

    await vi.advanceTimersByTimeAsync(500);
    expect(sockets).toHaveLength(4);
    sockets[3].open();
    const fourthFrames = await handshakeFrames(sockets[3], tickets[3], 0, 0);
    sockets[3].receive(fourthFrames.receipt);
    sockets[3].receiveRaw(fourthFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    expect(sockets[3].sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(499);
    expect(sockets[3].sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await vi.waitFor(() => expect(sockets[3].sent).toHaveLength(2));
    const backedOffReplay = await sentAuthenticatedCommand(sockets[3], fourthFrames);
    expect(backedOffReplay).toEqual(firstCommand);

    await receiveAuthenticated(sockets[3], fourthFrames, {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: backedOffReplay.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
      deduplicated: true,
    });
    await expect(pendingCommand).resolves.toMatchObject({
      accepted: true,
      resolution: "committed",
      deduplicated: true,
    });
    handle.close();
  });

  it("does not project an old socket after asynchronous snapshot verification", async () => {
    const onOpen = vi.fn();
    const onSnapshot = vi.fn();
    const { handle, sockets, tickets } = openHarness({
      onOpen,
      onRoomSnapshot: onSnapshot,
    });
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    await flushPromises();

    let releaseDigest = () => {};
    let reportDigestStarted = () => {};
    const digestStarted = new Promise<void>((resolve) => { reportDigestStarted = resolve; });
    const digestGate = new Promise<void>((resolve) => { releaseDigest = resolve; });
    const realDigest = crypto.subtle.digest.bind(crypto.subtle);
    vi.spyOn(crypto.subtle, "digest").mockImplementationOnce(async (algorithm, data) => {
      reportDigestStarted();
      await digestGate;
      return realDigest(algorithm, data);
    });

    sockets[0].receiveRaw(frames.rawSnapshot);
    await digestStarted;
    sockets[0].close();
    releaseDigest();
    await flushPromises();

    expect(onSnapshot).not.toHaveBeenCalled();
    expect(onOpen).not.toHaveBeenCalled();
    expect(handle.ready()).toBe(false);
    handle.close();
  });

  it("does not transmit a command whose pre-send deadline expired during signing", async () => {
    vi.useFakeTimers();
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    let releaseSignature = () => {};
    let reportSignatureStarted = () => {};
    const signatureStarted = new Promise<void>((resolve) => { reportSignatureStarted = resolve; });
    const signatureGate = new Promise<void>((resolve) => { releaseSignature = resolve; });
    const realSign = crypto.subtle.sign.bind(crypto.subtle);
    vi.spyOn(crypto.subtle, "sign").mockImplementationOnce(async (algorithm, key, data) => {
      reportSignatureStarted();
      await signatureGate;
      return realSign(algorithm, key, data);
    });

    const command = handle.command("message.send", { content: "never sent" });
    const rejection = expect(command).rejects.toMatchObject({ category: "timeout" });
    await signatureStarted;
    await vi.advanceTimersByTimeAsync(ROOM_SOCKET_COMMAND_TIMEOUT_MS_FOR_TEST);
    await rejection;
    releaseSignature();
    await flushPromises();

    expect(sockets[0].sent).toHaveLength(1);
    handle.close();
  });
});
