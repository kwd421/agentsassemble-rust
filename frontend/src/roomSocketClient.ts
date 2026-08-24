import {
  getWsTicket,
  type RoomEvent,
  type RoomSocketAuth,
} from "./api";
import {
  agentCreationProjectionFromEvent,
  joinedParticipantFromEvent,
} from "./lib/participantEventContract";
import {
  SubscriptionContractError,
  type SubscriptionReceipt,
  verifyBoundSnapshot,
  verifySubscriptionReceipt,
} from "./lib/roomSubscriptionContract";
import { createServerChallenge, isHex32Bytes } from "./lib/serverProof";
import {
  RoomSocketSayError,
  type ProviderCatalogSnapshot,
  type RoomCommandAck,
  type RoomSocketClientDependencies,
  type RoomSocketHandle,
  type RoomSocketHandlers,
  type RoomSocketSnapshot,
} from "./roomSocketTypes";

export type { RoomSocketAuth } from "./api";
export type { PluginEnvelope } from "./pluginSocketProtocol";
export { RoomSocketSayError } from "./roomSocketTypes";
export type {
  NativeCliProviderAvailability,
  ProviderCatalogSnapshot,
  ProviderControl,
  ProviderControlOption,
  RoomCommandAck,
  RoomHistoryPage,
  RoomSayRequest,
  RoomSocketClientDependencies,
  RoomSocketHandle,
  RoomSocketHandlers,
  RoomSocketSnapshot,
} from "./roomSocketTypes";

const ROOM_SOCKET_COMMAND_TIMEOUT_MS = 20_000;

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function isSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function participantProjectionIsValid(event: RoomEvent): boolean {
  try {
    if (event.type === "participant_joined") joinedParticipantFromEvent(event);
    if (event.type === "agent_session_created") agentCreationProjectionFromEvent(event);
    return true;
  } catch {
    return false;
  }
}

function commandAckResultIsValid(
  action: string,
  payload: Record<string, unknown>,
  result: unknown
): boolean {
  if (!isRecord(result)) return false;
  const event = isRecord(result.event) ? result.event : null;
  const hasDurableEvent = Boolean(
    event &&
    typeof event.id === "string" &&
    event.id &&
    isSequence(event.seq) &&
    event.seq > 0 &&
    result.event_seq === event.seq
  );
  if (action === "message.send" || action.startsWith("room.random.")) {
    return hasDurableEvent && event?.type === "message_final";
  }
  if (action === "message.edit" || action === "message.delete") {
    return Boolean(
      hasDurableEvent &&
      event?.type === (action === "message.edit" ? "message_updated" : "message_deleted") &&
      event?.target_event_id === payload.event_id
    );
  }
  if (action === "room.history") {
    return Boolean(
      Array.isArray(result.events) &&
      isSequence(result.oldest_seq) &&
      isSequence(result.last_seq) &&
      typeof result.has_more_before === "boolean"
    );
  }
  if (action === "participant.kick") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      participant.participant_id === payload.participant_id &&
      participant.status === "kicked"
    );
  }
  if (action === "participant.role.update") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      event &&
      participant.participant_id === payload.participant_id &&
      participant.role === payload.role &&
      event.type === "participant_updated" &&
      event.participant_id === payload.participant_id &&
      event.role === payload.role
    );
  }
  if (action === "room.settings.update") {
    return Boolean(isRecord(result.room_settings) && event?.type === "room_settings_updated");
  }
  if (action === "participant.mute") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      participant.participant_id === payload.participant_id &&
      participant.muted === Boolean(payload.muted)
    );
  }
  if (action === "participant.leave") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      participant.status === "left" &&
      event?.type === "participant_left"
    );
  }
  if (action === "room.delete") return result.deleted === true;
  if (action === "provider.request.resolve") {
    return Boolean(
      result.status === "resolving" &&
      result.provider_request_id === payload.provider_request_id &&
      event?.type === "provider_request_resolution_requested"
    );
  }
  if (action === "agent.create" || action === "agent.configure") {
    return isRecord(result.agent_session);
  }
  if (action === "agent.readd") return result.status === "readded";
  if (action.startsWith("agent.")) return isRecord(result.agent_session);
  if (action === "room.vote.summary") {
    return Boolean(
      typeof result.question === "string" &&
      isRecord(result.tallies) &&
      isSequence(result.total_votes)
    );
  }
  return true;
}

function snapshotValidationError(
  value: unknown,
  {
    expectedRoomId,
    currentLastSeq,
  }: { expectedRoomId: string; currentLastSeq: number }
): RoomSocketSayError | null {
  if (
    !isRecord(value) ||
    value.op !== "snapshot" ||
    value.stream !== "room_events" ||
    !isRecord(value.room) ||
    value.room.room_id !== expectedRoomId ||
    !isRecord(value.room_settings) ||
    !Array.isArray(value.participants) ||
    !Array.isArray(value.agent_sessions) ||
    !Array.isArray(value.active_turns) ||
    !Array.isArray(value.events) ||
    !isRecord(value.provider_catalog) ||
    !Array.isArray(value.available_providers) ||
    !isRecord(value.capabilities) ||
    typeof value.has_more_before !== "boolean" ||
    typeof value.resume_gap !== "boolean"
  ) {
    return new RoomSocketSayError(
      "Room snapshot did not match the canonical browser schema; reconnecting.",
      "snapshot_schema_invalid"
    );
  }
  const mode = value.snapshot_mode;
  if (mode !== "initial" && mode !== "resume" && mode !== "gap") {
    return new RoomSocketSayError(
      "Room snapshot used an invalid browser snapshot mode; reconnecting.",
      "snapshot_mode_invalid"
    );
  }
  if (
    value.resume_gap !== (mode === "gap") ||
    (mode === "initial" && currentLastSeq !== 0) ||
    (mode !== "initial" && currentLastSeq <= 0) ||
    !isSequence(value.oldest_seq) ||
    !isSequence(value.last_seq) ||
    value.last_seq < currentLastSeq
  ) {
    return new RoomSocketSayError(
      "Room snapshot sequence metadata was inconsistent; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  const sequences: number[] = [];
  for (const event of value.events) {
    if (
      !isRecord(event) ||
      typeof event.id !== "string" ||
      !event.id ||
      event.room_id !== expectedRoomId ||
      typeof event.type !== "string" ||
      !event.type ||
      !isSequence(event.seq) ||
      event.seq <= 0 ||
      !participantProjectionIsValid(event as unknown as RoomEvent)
    ) {
      return new RoomSocketSayError(
        "Room snapshot contained an invalid canonical event; reconnecting.",
        "snapshot_event_invalid"
      );
    }
    sequences.push(event.seq);
  }
  if (sequences.some((sequence, index) => index > 0 && sequence !== sequences[index - 1] + 1)) {
    return new RoomSocketSayError(
      "Room snapshot event sequence was not contiguous; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  if (!sequences.length) {
    const validEmptyBoundary =
      value.oldest_seq === 0 &&
      ((mode === "initial" && value.last_seq === 0) ||
        (mode === "resume" && value.last_seq === currentLastSeq));
    return validEmptyBoundary
      ? null
      : new RoomSocketSayError(
          "Room snapshot omitted events required by its sequence boundary; reconnecting.",
          "snapshot_sequence_invalid"
        );
  }
  const firstSeq = sequences[0];
  const finalSeq = sequences[sequences.length - 1];
  if (
    value.oldest_seq !== firstSeq ||
    value.last_seq !== finalSeq ||
    (mode === "resume" && firstSeq !== currentLastSeq + 1)
  ) {
    return new RoomSocketSayError(
      "Room snapshot event range did not match its durable cursor; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  return null;
}

function validateClientAuthority(
  streams: readonly string[],
  dependencies: RoomSocketClientDependencies
) {
  const surface = dependencies.serverSurface;
  if (
    surface.revision !== 1 ||
    !isHex32Bytes(surface.digest) ||
    !dependencies.expectedRoomId ||
    !dependencies.expectedParticipantId ||
    streams.length !== 1 ||
    streams[0] !== "room_events" ||
    surface.websocket_streams.length !== 1 ||
    surface.websocket_streams[0] !== "room_events"
  ) {
    throw new RoomSocketSayError(
      "The room transport is not bound to the canonical server product surface.",
      "surface_stream_unavailable"
    );
  }
}

function ticketSocketUrl(websocketBaseUrl: string, ticket: string): string {
  const base = new URL(websocketBaseUrl);
  if ((base.protocol !== "ws:" && base.protocol !== "wss:") || base.username || base.password) {
    throw new RoomSocketSayError(
      "The desktop runtime returned an invalid WebSocket authority.",
      "websocket_authority_invalid"
    );
  }
  const url = new URL("/ws", base);
  url.searchParams.set("ticket", ticket);
  return url.toString();
}

/**
 * Opens the canonical room transport. The socket becomes ready only after a signed
 * receipt, its exact snapshot bytes, and the finite C+1..H catch-up have all verified.
 */
export function openRoomSocket(
  auth: RoomSocketAuth,
  streams: string[],
  handlers: RoomSocketHandlers,
  dependencies: RoomSocketClientDependencies
): RoomSocketHandle {
  let socket: WebSocket | null = null;
  let closed = false;
  let reconnectTimer = 0;
  let reconnectAttempt = 0;
  let lastSeq = 0;
  let requestCounter = 0;
  let transportReady = false;
  const requestTicket = dependencies.getTicket || getWsTicket;
  const createSocket = dependencies.createSocket || ((url: string) => new WebSocket(url));
  const pending = new Map<
    string,
    {
      action: string;
      payload: Record<string, unknown>;
      resolve: (value: RoomCommandAck) => void;
      reject: (reason: Error) => void;
      timerId: number;
    }
  >();

  function nextRequestId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
    requestCounter += 1;
    return `web-${Date.now().toString(36)}-${requestCounter.toString(36)}`;
  }

  function sendPending() {
    if (!socket || socket.readyState !== WebSocket.OPEN || !transportReady) return;
    pending.forEach((command, requestId) => {
      socket?.send(JSON.stringify({
        op: "command",
        request_id: requestId,
        action: command.action,
        payload: command.payload,
      }));
    });
  }

  function rejectAll(error: Error) {
    pending.forEach((command) => {
      window.clearTimeout(command.timerId);
      command.reject(error);
    });
    pending.clear();
  }

  function fail(currentSocket: WebSocket, error: unknown) {
    const normalized =
      error instanceof SubscriptionContractError
        ? new RoomSocketSayError(error.message, error.code)
        : error instanceof RoomSocketSayError
          ? error
          : error instanceof SyntaxError
            ? new RoomSocketSayError(
                "Room socket received malformed JSON; reconnecting.",
                "frame_json_invalid"
              )
            : error instanceof Error
              ? error
              : new Error("Room WebSocket protocol failed.");
    handlers.onError?.(normalized);
    if (socket === currentSocket) currentSocket.close();
  }

  function acceptEventFrame(
    currentSocket: WebSocket,
    msg: Record<string, unknown>,
    highWater: number,
    establishing: boolean
  ) {
    if (
      msg.op !== "event" ||
      msg.stream !== "room_events" ||
      !Array.isArray(msg.events) ||
      !isSequence(msg.latest_seq)
    ) {
      throw new RoomSocketSayError(
        "The subscription catch-up contained an invalid event frame.",
        "event_frame_invalid"
      );
    }
    const freshEvents: RoomEvent[] = [];
    let nextSeq = lastSeq;
    for (const rawEvent of msg.events) {
      if (
        !isRecord(rawEvent) ||
        typeof rawEvent.id !== "string" ||
        !rawEvent.id ||
        rawEvent.room_id !== dependencies.expectedRoomId ||
        typeof rawEvent.type !== "string" ||
        !rawEvent.type ||
        !isSequence(rawEvent.seq) ||
        rawEvent.seq <= 0 ||
        !participantProjectionIsValid(rawEvent as unknown as RoomEvent)
      ) {
        throw new RoomSocketSayError(
          "Room event did not match the canonical event schema; reconnecting.",
          "event_schema_invalid"
        );
      }
      if (establishing && rawEvent.seq <= nextSeq) {
        throw new RoomSocketSayError(
          "Subscription catch-up repeated an already delivered event.",
          "subscription_catchup_invalid"
        );
      }
      if (rawEvent.seq <= nextSeq) continue;
      if (rawEvent.seq !== nextSeq + 1) {
        throw new RoomSocketSayError(
          `Room event sequence gap detected (expected ${nextSeq + 1}, received ${rawEvent.seq}); reconnecting.`,
          "event_sequence_gap"
        );
      }
      if (establishing && rawEvent.seq > highWater) {
        throw new RoomSocketSayError(
          "Subscription catch-up exceeded its authenticated high-water mark.",
          "subscription_catchup_overflow"
        );
      }
      freshEvents.push(rawEvent as unknown as RoomEvent);
      nextSeq = rawEvent.seq;
    }
    if (!freshEvents.length || msg.latest_seq !== nextSeq) {
      throw new RoomSocketSayError(
        "Room event frame did not match its advertised durable cursor.",
        "event_sequence_invalid"
      );
    }
    lastSeq = nextSeq;
    handlers.onRoomEvents?.(freshEvents);
    if (socket !== currentSocket) throw new Error("stale room socket");
  }

  async function connect() {
    try {
      validateClientAuthority(streams, dependencies);
      const issued = await requestTicket(auth);
      if (closed) return;
      if (
        !issued ||
        typeof issued.ticket !== "string" ||
        !isHex32Bytes(issued.ticket) ||
        typeof issued.websocket_base_url !== "string" ||
        !isHex32Bytes(issued.server_proof_key)
      ) {
        throw new RoomSocketSayError(
          "The desktop runtime ticket contract is incomplete.",
          "runtime_ticket_invalid"
        );
      }
      const serverChallenge = createServerChallenge();
      const currentSocket = createSocket(ticketSocketUrl(issued.websocket_base_url, issued.ticket));
      socket = currentSocket;
      transportReady = false;
      let receipt: SubscriptionReceipt | null = null;
      let snapshotAccepted = false;
      let verificationQueue = Promise.resolve();

      const markReady = () => {
        if (
          transportReady ||
          !receipt ||
          !snapshotAccepted ||
          lastSeq !== receipt.catchup_high_water
        ) return;
        transportReady = true;
        reconnectAttempt = 0;
        handlers.onOpen?.();
        sendPending();
      };

      const processFrame = async (raw: string) => {
        const msg = JSON.parse(raw) as unknown;
        if (!receipt) {
          receipt = await verifySubscriptionReceipt(msg, {
            ticket: issued.ticket,
            proofKey: issued.server_proof_key,
            serverChallenge,
            roomId: dependencies.expectedRoomId,
            participantId: dependencies.expectedParticipantId,
            streams,
            serverSurface: dependencies.serverSurface,
          });
          return;
        }
        if (!snapshotAccepted) {
          await verifyBoundSnapshot(raw, msg, receipt);
          const validationError = snapshotValidationError(msg, {
            expectedRoomId: dependencies.expectedRoomId,
            currentLastSeq: lastSeq,
          });
          if (validationError) throw validationError;
          const snapshot = msg as RoomSocketSnapshot;
          if (handlers.onRoomSnapshot?.(snapshot) === false) {
            throw new RoomSocketSayError(
              "The room projection rejected its authenticated snapshot.",
              "snapshot_rejected"
            );
          }
          lastSeq = snapshot.last_seq;
          snapshotAccepted = true;
          markReady();
          return;
        }
        if (!isRecord(msg)) {
          throw new RoomSocketSayError("Room socket frame was invalid.", "frame_schema_invalid");
        }
        if (!transportReady) {
          acceptEventFrame(currentSocket, msg, receipt.catchup_high_water, true);
          markReady();
          return;
        }
        if (msg.op === "event") {
          acceptEventFrame(currentSocket, msg, Number.MAX_SAFE_INTEGER, false);
          return;
        }
        if (msg.op === "provider_catalog_updated" && isRecord(msg.catalog)) {
          handlers.onProviderCatalog?.(msg.catalog as unknown as ProviderCatalogSnapshot);
          return;
        }
        if (msg.op === "resync_required") {
          throw new RoomSocketSayError(
            String(msg.reason || "Room event delivery fell behind; reconnecting."),
            "resync_required"
          );
        }
        if ((msg.op === "ack" || msg.op === "nack") && typeof msg.request_id === "string") {
          const command = pending.get(msg.request_id);
          if (!command) return;
          if (msg.op === "nack" || msg.accepted === false) {
            pending.delete(msg.request_id);
            window.clearTimeout(command.timerId);
            const error = isRecord(msg.error) ? msg.error : {};
            command.reject(new RoomSocketSayError(
              String(error.message || "Room command was rejected."),
              String(error.code || "rejected")
            ));
            return;
          }
          if (
            msg.op !== "ack" ||
            msg.accepted !== true ||
            msg.action !== command.action ||
            !commandAckResultIsValid(command.action, command.payload, msg.result)
          ) {
            throw new RoomSocketSayError(
              "Room ACK did not match its pending command contract; reconnecting.",
              "ack_contract_invalid"
            );
          }
          pending.delete(msg.request_id);
          window.clearTimeout(command.timerId);
          command.resolve(msg as unknown as RoomCommandAck);
          return;
        }
        if (msg.op === "pong") return;
        throw new RoomSocketSayError(
          "Room socket returned a frame outside the bound product protocol.",
          "frame_unexpected"
        );
      };

      currentSocket.onopen = () => {
        currentSocket.send(JSON.stringify({
          op: "subscribe",
          streams,
          resume_from_seq: lastSeq,
          server_challenge: serverChallenge,
        }));
      };
      currentSocket.onmessage = (event) => {
        const raw = event.data;
        if (typeof raw !== "string") {
          fail(currentSocket, new RoomSocketSayError(
            "Binary WebSocket frames are not supported.",
            "binary_frame_unsupported"
          ));
          return;
        }
        verificationQueue = verificationQueue
          .then(async () => {
            if (socket !== currentSocket || currentSocket.readyState !== WebSocket.OPEN) return;
            await processFrame(raw);
          })
          .catch((error) => fail(currentSocket, error));
      };
      currentSocket.onerror = (event) => handlers.onError?.(event);
      currentSocket.onclose = () => {
        if (socket === currentSocket) socket = null;
        transportReady = false;
        handlers.onClose?.();
        if (closed) return;
        reconnectAttempt += 1;
        const delay = Math.min(5_000, 250 * 2 ** Math.min(reconnectAttempt, 5));
        reconnectTimer = window.setTimeout(() => void connect(), delay);
      };
    } catch (error) {
      handlers.onError?.(error as Error);
      if (!closed) {
        reconnectAttempt += 1;
        const delay = Math.min(5_000, 250 * 2 ** Math.min(reconnectAttempt, 5));
        reconnectTimer = window.setTimeout(() => void connect(), delay);
      }
    }
  }

  function command(action: string, payload: Record<string, unknown> = {}) {
    return new Promise<RoomCommandAck>((resolve, reject) => {
      if (closed) {
        reject(new RoomSocketSayError("Room socket is closed.", "socket_closed"));
        return;
      }
      if (!(dependencies.serverSurface.websocket_actions as readonly string[]).includes(action)) {
        reject(new RoomSocketSayError(
          `Room action ${action} is not present in the bound server product surface.`,
          "surface_action_unavailable"
        ));
        return;
      }
      const requestId = nextRequestId();
      const timerId = window.setTimeout(() => {
        const waiting = pending.get(requestId);
        if (!waiting) return;
        pending.delete(requestId);
        waiting.reject(new RoomSocketSayError("Room command timed out.", "timeout"));
      }, ROOM_SOCKET_COMMAND_TIMEOUT_MS);
      pending.set(requestId, { action, payload, resolve, reject, timerId });
      sendPending();
    });
  }

  void connect();

  return {
    close: () => {
      closed = true;
      transportReady = false;
      window.clearTimeout(reconnectTimer);
      rejectAll(new RoomSocketSayError("Room socket closed.", "socket_closed"));
      socket?.close();
    },
    resync: () => socket?.close(),
    ready: () => transportReady,
    command,
    historyBefore: async (beforeSeq, limit = 200) => {
      const ack = await command("room.history", { before_seq: beforeSeq, limit });
      const result = ack.result || {};
      return {
        events: Array.isArray(result.events) ? (result.events as RoomEvent[]) : [],
        oldest_seq: Number(result.oldest_seq || 0),
        last_seq: Number(result.last_seq || 0),
        has_more_before: Boolean(result.has_more_before),
      };
    },
    say: async (request) => {
      await command("message.send", { content: request.message });
      return { events: [] };
    },
  };
}
