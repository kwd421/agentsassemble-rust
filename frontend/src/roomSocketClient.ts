import {
  getWsTicket,
  type RoomEvent,
  type RoomSocketAuth,
} from "./api";
import {
  SubscriptionContractError,
  type SubscriptionReceipt,
  verifyBoundSnapshot,
  verifySubscriptionReceipt,
} from "./lib/roomSubscriptionContract";
import {
  decodeAuthenticatedFrame,
  deriveAuthenticatedFrameKey,
  encodeAuthenticatedFrame,
} from "./lib/authenticatedFrames";
import {
  commandAckResultIsValid,
  isRecord,
  isSequence,
  participantProjectionIsValid,
  snapshotValidationError,
} from "./lib/roomSocketValidation";
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
const ROOM_SOCKET_UNRESOLVED_RETRY_BASE_MS = 500;
const ROOM_SOCKET_UNRESOLVED_RETRY_MAX_MS = 30_000;
const MAX_ROOM_SOCKET_WIRE_CHARS = 384 * 1024;

interface PendingRoomCommand {
  action: string;
  payload: Record<string, unknown>;
  encoded: string;
  resolve: (value: RoomCommandAck) => void;
  reject: (reason: Error) => void;
  timerId: number | null;
  retryTimerId: number | null;
  retryAttempt: number;
  retryNotBefore: number;
  everSent: boolean;
}

function validateClientAuthority(
  streams: readonly string[],
  dependencies: RoomSocketClientDependencies
) {
  const surface = dependencies.serverSurface;
  if (
    surface.revision !== 2 ||
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
  let sendPendingForConnection: (() => void) | null = null;
  let connectionGeneration = 0;
  const requestTicket = dependencies.getTicket || getWsTicket;
  const createSocket = dependencies.createSocket || ((url: string) => new WebSocket(url));
  const pending = new Map<string, PendingRoomCommand>();

  function nextRequestId() {
    if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
      return crypto.randomUUID();
    }
    requestCounter += 1;
    return `web-${Date.now().toString(36)}-${requestCounter.toString(36)}`;
  }

  function sendPending() {
    sendPendingForConnection?.();
  }

  function rejectAll(error: Error) {
    pending.forEach((command) => {
      if (command.timerId !== null) window.clearTimeout(command.timerId);
      if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
      command.reject(command.everSent
        ? new RoomSocketSayError(
            "The room command outcome could not be confirmed before the socket closed.",
            "outcome_unknown"
          )
        : error);
    });
    pending.clear();
  }

  function armCommandDeadline(
    requestId: string,
    command: PendingRoomCommand
  ) {
    if (command.timerId !== null) window.clearTimeout(command.timerId);
    command.timerId = window.setTimeout(() => {
      if (pending.get(requestId) !== command) return;
      command.timerId = null;
      if (command.everSent) {
        const currentSocket = socket;
        if (currentSocket && currentSocket.readyState !== WebSocket.CLOSED) {
          currentSocket.close();
        }
        return;
      }
      pending.delete(requestId);
      command.reject(new RoomSocketSayError(
        "Room command timed out before it could be sent.",
        "timeout"
      ));
    }, ROOM_SOCKET_COMMAND_TIMEOUT_MS);
  }

  function fail(currentSocket: WebSocket, error: unknown) {
    if (socket !== currentSocket) return;
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
    const generation = ++connectionGeneration;
    try {
      validateClientAuthority(streams, dependencies);
      const issued = await requestTicket(auth);
      if (closed || generation !== connectionGeneration) return;
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
      let outboundQueue = Promise.resolve();
      let frameKey: CryptoKey | null = null;
      let nextServerCounter = 1;
      let nextClientCounter = 1;
      const sentRequestIds = new Set<string>();
      const isCurrentConnection = () =>
        !closed &&
        generation === connectionGeneration &&
        socket === currentSocket &&
        currentSocket.readyState === WebSocket.OPEN;

      sendPendingForConnection = () => {
        if (
          socket !== currentSocket ||
          currentSocket.readyState !== WebSocket.OPEN ||
          !transportReady ||
          !frameKey
        ) return;
        pending.forEach((command, requestId) => {
          if (sentRequestIds.has(requestId)) return;
          const retryDelay = command.retryNotBefore - Date.now();
          if (retryDelay > 0) {
            if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
            command.retryTimerId = window.setTimeout(() => {
              command.retryTimerId = null;
              sendPending();
            }, retryDelay);
            return;
          }
          if (command.retryTimerId !== null) {
            window.clearTimeout(command.retryTimerId);
            command.retryTimerId = null;
          }
          sentRequestIds.add(requestId);
          outboundQueue = outboundQueue
            .then(async () => {
              if (
                !isCurrentConnection() ||
                !transportReady ||
                !frameKey ||
                pending.get(requestId) !== command
              ) return;
              const encoded = await encodeAuthenticatedFrame(
                frameKey,
                receipt?.connection_nonce || "",
                "client",
                nextClientCounter,
                command.encoded
              );
              if (!isCurrentConnection() || pending.get(requestId) !== command) return;
              currentSocket.send(encoded);
              nextClientCounter += 1;
              command.everSent = true;
              armCommandDeadline(requestId, command);
            })
            .catch((error) => fail(currentSocket, error));
        });
      };

      const markReady = () => {
        if (
          !isCurrentConnection() ||
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
        if (raw.length > MAX_ROOM_SOCKET_WIRE_CHARS) {
          throw new RoomSocketSayError(
            "Room socket frame exceeded the product wire limit.",
            "frame_too_large"
          );
        }
        if (!receipt) {
          const msg = JSON.parse(raw) as unknown;
          const verifiedReceipt = await verifySubscriptionReceipt(msg, {
            ticket: issued.ticket,
            proofKey: issued.server_proof_key,
            serverChallenge,
            roomId: dependencies.expectedRoomId,
            participantId: dependencies.expectedParticipantId,
            streams,
            serverSurface: dependencies.serverSurface,
          });
          if (!isCurrentConnection()) return;
          const derivedFrameKey = await deriveAuthenticatedFrameKey(
            issued.server_proof_key,
            verifiedReceipt.connection_nonce
          );
          if (!isCurrentConnection()) return;
          receipt = verifiedReceipt;
          frameKey = derivedFrameKey;
          return;
        }
        if (!snapshotAccepted) {
          const msg = JSON.parse(raw) as unknown;
          await verifyBoundSnapshot(raw, msg, receipt);
          if (!isCurrentConnection()) return;
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
        if (!frameKey) {
          throw new RoomSocketSayError(
            "The authenticated room channel key is unavailable.",
            "frame_authentication_invalid"
          );
        }
        let payload: string;
        try {
          payload = await decodeAuthenticatedFrame(
            frameKey,
            receipt.connection_nonce,
            "server",
            nextServerCounter,
            raw
          );
        } catch {
          throw new RoomSocketSayError(
            "Room socket frame authentication failed; reconnecting.",
            "frame_authentication_invalid"
          );
        }
        if (!isCurrentConnection()) return;
        nextServerCounter += 1;
        const msg = JSON.parse(payload) as unknown;
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
          if (!command) {
            throw new RoomSocketSayError(
              "Room command response did not match an owned pending request; reconnecting.",
              "command_response_unexpected"
            );
          }
          if (msg.op === "nack") {
            if (
              msg.accepted !== false ||
              msg.action !== command.action ||
              (msg.resolution !== "rejected" && msg.resolution !== "unresolved") ||
              !isRecord(msg.error) ||
              typeof msg.error.code !== "string" ||
              typeof msg.error.message !== "string"
            ) {
              throw new RoomSocketSayError(
                "Room NACK did not carry a server-owned outcome resolution; reconnecting.",
                "nack_contract_invalid"
              );
            }
            if (msg.resolution === "unresolved") {
              if (command.timerId !== null) window.clearTimeout(command.timerId);
              command.timerId = null;
              command.retryAttempt += 1;
              const retryDelay = Math.min(
                ROOM_SOCKET_UNRESOLVED_RETRY_MAX_MS,
                ROOM_SOCKET_UNRESOLVED_RETRY_BASE_MS *
                  2 ** Math.min(command.retryAttempt - 1, 6)
              );
              command.retryNotBefore = Date.now() + retryDelay;
              currentSocket.close();
              return;
            }
            pending.delete(msg.request_id);
            if (command.timerId !== null) window.clearTimeout(command.timerId);
            if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
            command.reject(new RoomSocketSayError(
              msg.error.message,
              msg.error.code
            ));
            return;
          }
          if (
            msg.op !== "ack" ||
            msg.accepted !== true ||
            msg.resolution !== "committed" ||
            msg.action !== command.action ||
            !commandAckResultIsValid(command.action, command.payload, msg.result)
          ) {
            throw new RoomSocketSayError(
              "Room ACK did not match its pending command contract; reconnecting.",
              "ack_contract_invalid"
            );
          }
          pending.delete(msg.request_id);
          if (command.timerId !== null) window.clearTimeout(command.timerId);
          if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
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
        if (!isCurrentConnection()) return;
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
      currentSocket.onerror = (event) => {
        if (socket === currentSocket && generation === connectionGeneration) {
          handlers.onError?.(event);
        }
      };
      currentSocket.onclose = () => {
        if (socket !== currentSocket || generation !== connectionGeneration) return;
        socket = null;
        sendPendingForConnection = null;
        transportReady = false;
        handlers.onClose?.();
        if (closed) return;
        reconnectAttempt += 1;
        const delay = Math.min(5_000, 250 * 2 ** Math.min(reconnectAttempt, 5));
        reconnectTimer = window.setTimeout(() => void connect(), delay);
      };
    } catch (error) {
      if (generation !== connectionGeneration) return;
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
      const encoded = JSON.stringify({
        op: "command",
        request_id: requestId,
        action,
        payload,
      });
      const transmitted = JSON.parse(encoded) as { payload: Record<string, unknown> };
      const waiting = {
        action,
        payload: transmitted.payload,
        encoded,
        resolve,
        reject,
        timerId: null,
        retryTimerId: null,
        retryAttempt: 0,
        retryNotBefore: 0,
        everSent: false,
      };
      pending.set(requestId, waiting);
      armCommandDeadline(requestId, waiting);
      sendPending();
    });
  }

  void connect();

  return {
    close: () => {
      const closingSocket = socket;
      closed = true;
      connectionGeneration += 1;
      transportReady = false;
      sendPendingForConnection = null;
      socket = null;
      window.clearTimeout(reconnectTimer);
      rejectAll(new RoomSocketSayError("Room socket closed.", "socket_closed"));
      closingSocket?.close();
      if (closingSocket) handlers.onClose?.();
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
