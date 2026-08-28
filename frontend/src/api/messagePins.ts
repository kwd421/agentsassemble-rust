import {
  requestDesktopMessagePinsReadTicket,
  requestDesktopMessagePinsWriteTicket,
} from "../lib/desktopBridge";
import { canonicalRoomId } from "../lib/canonicalRoomId";
import {
  assertExactKeys,
  requiredString,
  strictRecord,
} from "../lib/strictJsonContract";
import {
  MAX_LOBBY_MESSAGE_PINS,
  MAX_MESSAGE_PIN_EVENT_ID_BYTES,
} from "../types/generated/MESSAGE_PINS_WIRE";
import {
  exchangeSessionHttpTicket,
  queryString,
  responseError,
} from "./http";

export type MessagePin = {
  event_id: string;
  channel_id: "lobby";
  pinned_at: string;
  seq: number;
  author: string;
  content: string;
  created_at: string;
  attachment_filenames: string[];
};

export type MessagePinsAuthority =
  | { kind: "local" }
  | { kind: "remote"; sessionToken: string };

type PinOperation = "read" | "write";

const PIN_KEYS = [
  "event_id",
  "channel_id",
  "pinned_at",
  "seq",
  "author",
  "content",
  "created_at",
  "attachment_filenames",
] as const;

function invalidResponse(): never {
  throw new Error("로비 메시지 핀 응답 계약이 올바르지 않습니다.");
}

function isUnicodeScalarString(value: string): boolean {
  for (const character of value) {
    const first = character.charCodeAt(0);
    if (character.length === 1 && first >= 0xd800 && first <= 0xdfff) return false;
  }
  return true;
}

function canonicalEventId(value: unknown): string {
  if (
    typeof value !== "string" ||
    !value ||
    !isUnicodeScalarString(value) ||
    value.includes("\0") ||
    new TextEncoder().encode(value).byteLength > MAX_MESSAGE_PIN_EVENT_ID_BYTES
  ) {
    throw new Error("메시지 식별자가 올바르지 않습니다.");
  }
  return value;
}

function timestamp(value: unknown): string {
  if (typeof value !== "string" || !value || Number.isNaN(Date.parse(value))) {
    invalidResponse();
  }
  return value;
}

function hasVisibleText(value: string): boolean {
  return [...value].some(
    (character) => !/[\p{White_Space}\p{Cc}\p{Cf}]/u.test(character)
  );
}

function parsePin(value: unknown): MessagePin {
  const pin = strictRecord(value, "로비 메시지 핀");
  assertExactKeys(pin, PIN_KEYS, "로비 메시지 핀");
  const eventId = canonicalEventId(pin.event_id);
  const author = requiredString(pin, "author", "로비 메시지 핀");
  const content = requiredString(pin, "content", "로비 메시지 핀");
  if (
    !isUnicodeScalarString(author) ||
    !isUnicodeScalarString(content) ||
    !hasVisibleText(content) ||
    pin.channel_id !== "lobby" ||
    !Number.isSafeInteger(pin.seq) ||
    Number(pin.seq) < 1 ||
    !Array.isArray(pin.attachment_filenames) ||
    pin.attachment_filenames.length !== 0
  ) {
    invalidResponse();
  }
  return Object.freeze({
    event_id: eventId,
    channel_id: "lobby",
    pinned_at: timestamp(pin.pinned_at),
    seq: pin.seq as number,
    author,
    content,
    created_at: timestamp(pin.created_at),
    attachment_filenames: [],
  });
}

function parsePins(value: unknown): MessagePin[] {
  if (!Array.isArray(value) || value.length > MAX_LOBBY_MESSAGE_PINS) {
    invalidResponse();
  }
  const pins = value.map(parsePin);
  if (
    new Set(pins.map((pin) => pin.event_id)).size !== pins.length ||
    new Set(pins.map((pin) => pin.seq)).size !== pins.length
  ) {
    invalidResponse();
  }
  return pins;
}

function parseListResponse(value: unknown): MessagePin[] {
  const response = strictRecord(value, "로비 메시지 핀 목록");
  assertExactKeys(response, ["pins"], "로비 메시지 핀 목록");
  return parsePins(response.pins);
}

function parseMutationResponse(
  value: unknown,
  expectedEventId: string,
  expectedPinned: boolean
): MessagePin[] {
  const response = strictRecord(value, "로비 메시지 핀 변경");
  assertExactKeys(response, ["pinned", "pins"], "로비 메시지 핀 변경");
  if (response.pinned !== expectedPinned) invalidResponse();
  const pins = parsePins(response.pins);
  if (pins.some((pin) => pin.event_id === expectedEventId) !== expectedPinned) {
    invalidResponse();
  }
  return pins;
}

async function operationGrant(
  roomId: string,
  authority: MessagePinsAuthority,
  operation: PinOperation
): Promise<{ baseUrl: string; ticket: string }> {
  if (authority.kind === "local") {
    const grant =
      operation === "read"
        ? await requestDesktopMessagePinsReadTicket(roomId)
        : await requestDesktopMessagePinsWriteTicket(roomId);
    return { baseUrl: grant.http_base_url, ticket: grant.ticket };
  }
  if (!authority.sessionToken) {
    throw new Error("방 세션 권위를 사용할 수 없습니다.");
  }
  return {
    baseUrl: "",
    ticket: await exchangeSessionHttpTicket(
      operation === "read" ? "message-pins-read" : "message-pins-write",
      authority.sessionToken
    ),
  };
}

function bearer(ticket: string, json = false): Headers {
  const headers = new Headers({ Authorization: `Bearer ${ticket}` });
  if (json) headers.set("Content-Type", "application/json");
  return headers;
}

export async function fetchLobbyMessagePins({
  roomId,
  authority,
  beforeDispatch,
}: {
  roomId: string;
  authority: MessagePinsAuthority;
  beforeDispatch?: () => void;
}): Promise<MessagePin[]> {
  const canonicalRoom = canonicalRoomId(roomId);
  const grant = await operationGrant(canonicalRoom, authority, "read");
  const path = `/api/room-pins${queryString({
    room_id: canonicalRoom,
    channel_id: "lobby",
  })}`;
  beforeDispatch?.();
  const response = await fetch(`${grant.baseUrl}${path}`, {
    cache: "no-store",
    headers: bearer(grant.ticket),
  });
  if (!response.ok) throw await responseError(response);
  return parseListResponse(await response.json());
}

export async function setLobbyMessagePinned({
  roomId,
  eventId,
  pinned,
  authority,
  beforeDispatch,
}: {
  roomId: string;
  eventId: string;
  pinned: boolean;
  authority: MessagePinsAuthority;
  beforeDispatch?: () => void;
}): Promise<MessagePin[]> {
  const canonicalRoom = canonicalRoomId(roomId);
  const canonicalEvent = canonicalEventId(eventId);
  const grant = await operationGrant(canonicalRoom, authority, "write");
  beforeDispatch?.();
  const response = await fetch(`${grant.baseUrl}/api/room-pins`, {
    cache: "no-store",
    method: "POST",
    headers: bearer(grant.ticket, true),
    body: JSON.stringify({
      room_id: canonicalRoom,
      channel_id: "lobby",
      event_id: canonicalEvent,
      pinned,
    }),
  });
  if (!response.ok) throw await responseError(response);
  return parseMutationResponse(await response.json(), canonicalEvent, pinned);
}
