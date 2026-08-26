import { vi } from "vitest";
import { openRoomSocket } from "../roomSocketClient";
import {
  deriveConnectionNonce,
  digestPermissions,
  digestSnapshotFrame,
  subscriptionProofTranscript,
} from "../lib/serverProof";
import { utf8 } from "../lib/lengthDelimitedCrypto";
import {
  decodeAuthenticatedFrame,
  deriveAuthenticatedFrameKey,
  encodeAuthenticatedFrame,
} from "../lib/authenticatedFrames";
import type { SubscriptionReceipt } from "../lib/roomSubscriptionContract";
import { TEST_SERVER_PRODUCT_SURFACE } from "./serverProductSurface";

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

export class FakeWebSocket {
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

export function event(seq: number) {
  return {
    v: 1,
    id: `evt-${seq}`,
    room_id: "general",
    seq,
    type: "message_final",
    content: `message ${seq}`,
  };
}

export function malformedRoleEvent(seq: number) {
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

export function malformedMuteEvent(seq: number) {
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

export async function signReceipt(receipt: SubscriptionReceipt): Promise<string> {
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

export async function handshakeFrames(
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

export async function receiveAuthenticated(
  socket: FakeWebSocket,
  frames: Awaited<ReturnType<typeof handshakeFrames>>,
  message: Record<string, unknown>
) {
  socket.receiveRaw(await authenticatedServerFrame(frames, message));
}

export async function authenticatedServerFrame(
  frames: Awaited<ReturnType<typeof handshakeFrames>>,
  message: Record<string, unknown>
) {
  frames.serverCounter += 1;
  return encodeAuthenticatedFrame(
    frames.frameKey,
    frames.connectionNonce,
    "server",
    frames.serverCounter,
    JSON.stringify(message)
  );
}

export function gateNextFrameVerification() {
  let release = () => {};
  let reportStarted = () => {};
  const started = new Promise<void>((resolve) => { reportStarted = resolve; });
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const realVerify = crypto.subtle.verify.bind(crypto.subtle);
  vi.spyOn(crypto.subtle, "verify").mockImplementationOnce(
    async (algorithm, key, signature, data) => {
      reportStarted();
      await gate;
      return realVerify(algorithm, key, signature, data);
    }
  );
  return { release, started };
}

export async function sentAuthenticatedCommand(
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

export function openHarness(handlers: Parameters<typeof openRoomSocket>[2] = {}) {
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

export async function flushPromises() {
  if (vi.isFakeTimers()) {
    await vi.advanceTimersByTimeAsync(0);
  } else {
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  }
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}
