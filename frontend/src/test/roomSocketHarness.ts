import { vi } from "vitest";
import { openRoomSocket } from "../roomSocketClient";
import type { SubscriptionReceipt } from "../lib/roomSubscriptionContract";
import { TEST_SERVER_PRODUCT_SURFACE } from "./serverProductSurface";

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

export async function handshakeFrames(
  snapshotCursor: number,
  catchupHighWater: number,
  mutateReceipt?: (receipt: SubscriptionReceipt) => void
) {
  const snap = snapshot(snapshotCursor);
  const rawSnapshot = JSON.stringify(snap);
  const receipt: SubscriptionReceipt = {
    op: "subscribed",
    streams: ["room_events"],
    protocol_version: 1,
    room_id: "general",
    principal_id: "operator",
    participant_id: "operator-local",
    server_surface_revision: TEST_SERVER_PRODUCT_SURFACE.revision,
    server_surface_digest: TEST_SERVER_PRODUCT_SURFACE.digest,
    snapshot_cursor: snapshotCursor,
    catchup_high_water: catchupHighWater,
  };
  mutateReceipt?.(receipt);
  return {
    receipt,
    snap,
    rawSnapshot,
  };
}

export async function receiveServerFrame(
  socket: FakeWebSocket,
  frames: Awaited<ReturnType<typeof handshakeFrames>>,
  message: Record<string, unknown>
) {
  socket.receiveRaw(await encodedServerFrame(frames, message));
}

export async function encodedServerFrame(
  _frames: Awaited<ReturnType<typeof handshakeFrames>>,
  message: Record<string, unknown>
) {
  return JSON.stringify(message);
}

export async function sentClientFrame(
  socket: FakeWebSocket,
  _frames: Awaited<ReturnType<typeof handshakeFrames>>,
  index = 1
) {
  return socket.sent[index];
}

export function openHarness(handlers: Parameters<typeof openRoomSocket>[2] = {}) {
  const sockets: FakeWebSocket[] = [];
  let issued = 0;
  let reportOpened = () => {};
  const opened = new Promise<void>((resolve) => { reportOpened = resolve; });
  const handle = openRoomSocket(
    { kind: "host", meetingId: "general" },
    ["room_events"],
    {
      ...handlers,
      onOpen: () => {
        handlers.onOpen?.();
        reportOpened();
      },
    },
    {
      getTicket: async () => {
        issued += 1;
        const ticket = issued.toString(16).padStart(64, "a");
        return {
          ticket,
          ttl_seconds: 30,
          websocket_base_url: "ws://127.0.0.1:43123",
          displayResourceBase: "http://127.0.0.1:43123",
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
  return { handle, opened, sockets };
}

export async function flushPromises() {
  if (vi.isFakeTimers()) {
    await vi.advanceTimersByTimeAsync(0);
  } else {
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  }
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}
