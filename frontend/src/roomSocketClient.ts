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
  publicRoomEventIsValid,
  snapshotValidationError,
} from "./lib/roomSocketValidation";
import { createServerChallenge, isHex32Bytes } from "./lib/serverProof";
import {
  RoomSocketSayError,
  type ProviderCatalogSnapshot,
  type RoomCommandAck,
  type RoomHistoryPage,
  type RoomSayRequest,
  type RoomSocketClientDependencies,
  type RoomSocketHandle,
  type RoomSocketHandlers,
  type RoomSocketSnapshot,
} from "./roomSocketTypes";
import { PRODUCT_SURFACE_REVISION } from "./types/generated/PRODUCT_SURFACE_REVISION";
import { requireAcceptedRoomRuntimeTicket } from "./lib/roomRuntimeTicket";
import { messageAttachmentId } from "./lib/messageAttachmentId";
import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "./types/generated/MESSAGE_ATTACHMENTS_WIRE";
import { ROOM_HISTORY_MAX_EVENTS } from "./types/generated/ROOM_HISTORY_WIRE";
import { scheduleUncertainCommandRetry, type PendingCommandRetryState } from "./roomSocketRetryPolicy";

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
const ROOM_SOCKET_KEEPALIVE_MS = 3 * 60_000;
const MAX_ROOM_SOCKET_WIRE_CHARS = 384 * 1024;

interface PendingRoomCommand extends PendingCommandRetryState {
  action: string;
  payload: Record<string, unknown>;
  encoded: string;
  resolve: (value: RoomCommandAck) => void;
  reject: (reason: Error) => void;
}

function messageAttachmentIds(request: RoomSayRequest): string[] {
  const attachments = request.attachments || [];
  if (attachments.length > MAX_MESSAGE_ATTACHMENTS_PER_EVENT) {
    throw new RoomSocketSayError(
      "A room message cannot contain more than eight attachments.",
      "bad_request"
    );
  }
  let ids: string[];
  try {
    ids = attachments.map((attachment) => messageAttachmentId(attachment.id));
  } catch {
    throw new RoomSocketSayError(
      "A room message contains an invalid attachment identifier.",
      "bad_request"
    );
  }
  if (new Set(ids).size !== ids.length) {
    throw new RoomSocketSayError(
      "A room message cannot contain duplicate attachments.",
      "bad_request"
    );
  }
  return ids;
}

function requireOrdinaryMessage(request: RoomSayRequest) {
  if (request.kind && request.kind !== "message") {
    throw new RoomSocketSayError(
      `Room message kind ${request.kind} is not present in the bound server product surface.`,
      "surface_action_unavailable"
    );
  }
}

function validateClientAuthority(
  streams: readonly string[],
  dependencies: RoomSocketClientDependencies
) {
  const surface = dependencies.serverSurface;
  if (
    surface.revision !== PRODUCT_SURFACE_REVISION ||
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
  let stopKeepaliveForConnection: (() => void) | null = null;
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

  function scheduleReconnect(generation: number) {
    if (closed || generation !== connectionGeneration) return;
    reconnectAttempt += 1;
    const delay = Math.min(5_000, 250 * 2 ** Math.min(reconnectAttempt, 5));
    reconnectTimer = window.setTimeout(() => void connect(), delay);
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

  function rejectUnknown(requestId: string, command: PendingRoomCommand) {
    pending.delete(requestId);
    if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
    command.reject(new RoomSocketSayError(
      "The room command outcome remained unresolved after bounded exact replay.",
      "outcome_unknown"
    ));
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
        if (scheduleUncertainCommandRetry(command, connectionGeneration) === "exhausted") {
          rejectUnknown(requestId, command);
        }
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

  function fail(currentSocket: WebSocket, generation: number, error: unknown) {
    if (closed || generation !== connectionGeneration) return;
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
    if (socket === currentSocket && currentSocket.readyState !== WebSocket.CLOSED) {
      currentSocket.close();
    }
  }

  function acceptEventFrame(
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
        !publicRoomEventIsValid(rawEvent, dependencies.expectedRoomId)
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
  }

  async function connect() {
    const generation = ++connectionGeneration;
    try {
      validateClientAuthority(streams, dependencies);
      const rawTicket = await requestTicket(auth);
      if (closed || generation !== connectionGeneration) return;
      let issued;
      try {
        issued = requireAcceptedRoomRuntimeTicket(rawTicket);
      } catch {
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
      let connectionEstablished = false;
      let connectionFailed = false;
      let terminalLeaveCommitted = false;
      let verificationQueue = Promise.resolve();
      let outboundQueue = Promise.resolve();
      let frameKey: CryptoKey | null = null;
      let nextServerCounter = 1;
      let nextClientCounter = 1;
      let keepaliveTimer = 0;
      let keepaliveSequence = 0;
      let expectedPongNonce: string | null = null;
      const ownsConnectionGeneration = () =>
        !closed && generation === connectionGeneration;
      const canUseOpenSocket = () =>
        ownsConnectionGeneration() &&
        socket === currentSocket &&
        currentSocket.readyState === WebSocket.OPEN;
      const failConnection = (error: unknown) => {
        if (connectionFailed) return;
        connectionFailed = true;
        fail(currentSocket, generation, error);
      };
      const clearKeepalive = () => {
        window.clearTimeout(keepaliveTimer);
        keepaliveTimer = 0;
      };
      const scheduleKeepalive = () => {
        clearKeepalive();
        if (!canUseOpenSocket() || !transportReady || !frameKey) return;
        keepaliveTimer = window.setTimeout(() => {
          keepaliveTimer = 0;
          if (expectedPongNonce !== null) {
            failConnection(new RoomSocketSayError(
              "Room socket did not acknowledge its authenticated keepalive.",
              "keepalive_response_missing"
            ));
            return;
          }
          const payload = JSON.stringify({
            op: "ping",
            nonce: `keepalive-${++keepaliveSequence}`,
          });
          outboundQueue = outboundQueue
            .then(async () => {
              if (!canUseOpenSocket() || !transportReady || !frameKey) return;
              const encoded = await encodeAuthenticatedFrame(
                frameKey,
                receipt?.connection_nonce || "",
                "client",
                nextClientCounter,
                payload
              );
              if (!canUseOpenSocket() || !transportReady) return;
              expectedPongNonce = `keepalive-${keepaliveSequence}`;
              currentSocket.send(encoded);
              nextClientCounter += 1;
              scheduleKeepalive();
            })
            .catch(failConnection);
        }, ROOM_SOCKET_KEEPALIVE_MS);
      };
      stopKeepaliveForConnection = clearKeepalive;

      sendPendingForConnection = () => {
        if (
          socket !== currentSocket ||
          currentSocket.readyState !== WebSocket.OPEN ||
          !transportReady ||
          !frameKey
        ) return;
        pending.forEach((command, requestId) => {
          if (command.transmissionGeneration === generation) return;
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
          command.transmissionGeneration = generation;
          command.transmissionPhase = "encoding";
          outboundQueue = outboundQueue
            .then(async () => {
              if (
                !canUseOpenSocket() ||
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
              if (!canUseOpenSocket() || pending.get(requestId) !== command) return;
              currentSocket.send(encoded);
              command.transmissionPhase = "sent";
              nextClientCounter += 1;
              scheduleKeepalive();
              command.everSent = true;
              armCommandDeadline(requestId, command);
            })
            .catch(failConnection);
        });
      };

      const markReady = () => {
        if (
          !canUseOpenSocket() ||
          transportReady ||
          !receipt ||
          !snapshotAccepted ||
          lastSeq !== receipt.catchup_high_water
        ) return;
        connectionEstablished = true;
        transportReady = true;
        reconnectAttempt = 0;
        scheduleKeepalive();
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
          if (!canUseOpenSocket()) return;
          const derivedFrameKey = await deriveAuthenticatedFrameKey(
            issued.server_proof_key,
            verifiedReceipt.connection_nonce
          );
          if (!canUseOpenSocket()) return;
          receipt = verifiedReceipt;
          frameKey = derivedFrameKey;
          return;
        }
        if (!snapshotAccepted) {
          const msg = JSON.parse(raw) as unknown;
          await verifyBoundSnapshot(raw, msg, receipt);
          if (!canUseOpenSocket()) return;
          const validationError = snapshotValidationError(msg, {
            expectedRoomId: dependencies.expectedRoomId,
            currentLastSeq: lastSeq,
          });
          if (validationError) throw validationError;
          const snapshot = msg as RoomSocketSnapshot;
          if (
            handlers.onRoomSnapshot?.(
              snapshot,
              issued.displayResourceBase
            ) === false
          ) {
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
        if (!ownsConnectionGeneration() || connectionFailed) return;
        nextServerCounter += 1;
        const msg = JSON.parse(payload) as unknown;
        if (!isRecord(msg)) {
          throw new RoomSocketSayError("Room socket frame was invalid.", "frame_schema_invalid");
        }
        if (!connectionEstablished) {
          acceptEventFrame(msg, receipt.catchup_high_water, true);
          markReady();
          return;
        }
        if (msg.op === "event") {
          acceptEventFrame(msg, Number.MAX_SAFE_INTEGER, false);
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
              const retry = scheduleUncertainCommandRetry(command, generation);
              if (retry === "exhausted") {
                rejectUnknown(msg.request_id, command);
                return;
              }
              if (retry === "already_counted") return;
              connectionFailed = true;
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
            !commandAckResultIsValid(
              command.action,
              command.payload,
              msg.result,
              dependencies.expectedRoomId,
              dependencies.expectedParticipantId
            )
          ) {
            throw new RoomSocketSayError(
              "Room ACK did not match its pending command contract; reconnecting.",
              "ack_contract_invalid"
            );
          }
          pending.delete(msg.request_id);
          if (command.timerId !== null) window.clearTimeout(command.timerId);
          if (command.retryTimerId !== null) window.clearTimeout(command.retryTimerId);
          if (command.action === "participant.leave") terminalLeaveCommitted = true;
          command.resolve(msg as unknown as RoomCommandAck);
          return;
        }
        if (msg.op === "pong") {
          if (msg.nonce !== expectedPongNonce) {
            throw new RoomSocketSayError(
              "Room socket returned an unexpected keepalive response.",
              "keepalive_response_invalid"
            );
          }
          expectedPongNonce = null;
          return;
        }
        throw new RoomSocketSayError(
          "Room socket returned a frame outside the bound product protocol.",
          "frame_unexpected"
        );
      };

      currentSocket.onopen = () => {
        if (!canUseOpenSocket()) return;
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
          failConnection(new RoomSocketSayError(
            "Binary WebSocket frames are not supported.",
            "binary_frame_unsupported"
          ));
          return;
        }
        verificationQueue = verificationQueue
          .then(async () => {
            if (
              !ownsConnectionGeneration() ||
              connectionFailed ||
              (!connectionEstablished && !canUseOpenSocket())
            ) return;
            await processFrame(raw);
          })
          .catch(failConnection);
      };
      currentSocket.onerror = (event) => {
        if (socket === currentSocket && generation === connectionGeneration) {
          handlers.onError?.(event);
        }
      };
      currentSocket.onclose = () => {
        clearKeepalive();
        if (stopKeepaliveForConnection === clearKeepalive) {
          stopKeepaliveForConnection = null;
        }
        if (socket !== currentSocket || generation !== connectionGeneration) return;
        socket = null;
        sendPendingForConnection = null;
        transportReady = false;
        handlers.onClose?.();
        if (closed) return;
        void verificationQueue.finally(() => {
          pending.forEach((command, requestId) => {
            if (
              command.transmissionGeneration !== generation ||
              command.transmissionPhase !== "sent"
            ) return;
            if (command.timerId !== null) window.clearTimeout(command.timerId);
            command.timerId = null;
            if (scheduleUncertainCommandRetry(command, generation) === "exhausted") {
              rejectUnknown(requestId, command);
            }
          });
          if (terminalLeaveCommitted) return;
          scheduleReconnect(generation);
        });
      };
    } catch (error) {
      if (generation !== connectionGeneration) return;
      handlers.onError?.(error as Error);
      scheduleReconnect(generation);
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
        retryCountedGeneration: 0,
        retryNotBefore: 0,
        transmissionGeneration: 0,
        transmissionPhase: "idle" as const,
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
      stopKeepaliveForConnection?.();
      stopKeepaliveForConnection = null;
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
    historyBefore: async (beforeSeq, limit = ROOM_HISTORY_MAX_EVENTS) => {
      const ack = await command("room.history", { before_seq: beforeSeq, limit });
      return ack.result as unknown as RoomHistoryPage;
    },
    say: async (request) => {
      requireOrdinaryMessage(request);
      const attachmentIds = messageAttachmentIds(request);
      await command("message.send", {
        content: request.message,
        ...(attachmentIds.length ? { attachment_ids: attachmentIds } : {}),
      });
      return { events: [] };
    },
  };
}
