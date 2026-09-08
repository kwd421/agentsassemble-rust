import { requireMessageChannelId } from "../lib/customChannelId";
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
import { isUnicodeScalarString } from "../lib/unicodeScalarString";
import {
  MAX_CHANNEL_MESSAGE_PINS,
  MAX_MESSAGE_EVENT_ID_BYTES,
} from "../types/generated/MESSAGE_PINS_WIRE";
import { queryString, responseError } from "./http";
import type { RoomHttpAuthority } from "./roomHttpAuthority";

export type MessagePin = {
  event_id: string;
  channel_id: string;
  pinned_at: string;
  seq: number;
  author: string;
  content: string;
  created_at: string;
  attachment_filenames: string[];
};

export type MessagePinsAuthority = RoomHttpAuthority;

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
  throw new Error("메시지 고정 응답 계약이 올바르지 않습니다.");
}

function canonicalEventId(value: unknown): string {
  if (
    typeof value !== "string" ||
    !value ||
    !isUnicodeScalarString(value) ||
    value.includes("\0") ||
    new TextEncoder().encode(value).byteLength > MAX_MESSAGE_EVENT_ID_BYTES
  ) {
    throw new Error("메시지 식별자가 올바르지 않습니다.");
  }
  return value;
}

function timestamp(value: unknown): string {
  if (
    typeof value !== "string" ||
    !value ||
    !isUnicodeScalarString(value) ||
    Number.isNaN(Date.parse(value))
  ) {
    invalidResponse();
  }
  return value;
}

function hasVisibleText(value: string): boolean {
  return [...value].some(
    (character) => !/[\p{White_Space}\p{Cc}\p{Cf}]/u.test(character)
  );
}

function parsePin(value: unknown, channelId: string): MessagePin {
  const pin = strictRecord(value, "메시지 고정");
  assertExactKeys(pin, PIN_KEYS, "메시지 고정");
  const eventId = canonicalEventId(pin.event_id);
  const author = requiredString(pin, "author", "메시지 고정");
  const content = requiredString(pin, "content", "메시지 고정");
  if (
    !isUnicodeScalarString(author) ||
    !isUnicodeScalarString(content) ||
    !hasVisibleText(content) ||
    pin.channel_id !== channelId ||
    !Number.isSafeInteger(pin.seq) ||
    Number(pin.seq) < 1 ||
    !Array.isArray(pin.attachment_filenames) ||
    pin.attachment_filenames.length !== 0
  ) {
    invalidResponse();
  }
  return Object.freeze({
    event_id: eventId,
    channel_id: channelId,
    pinned_at: timestamp(pin.pinned_at),
    seq: pin.seq as number,
    author,
    content,
    created_at: timestamp(pin.created_at),
    attachment_filenames: [],
  });
}

function parsePins(value: unknown, channelId: string): MessagePin[] {
  if (!Array.isArray(value) || value.length > MAX_CHANNEL_MESSAGE_PINS) {
    invalidResponse();
  }
  const pins = value.map((pin) => parsePin(pin, channelId));
  if (
    new Set(pins.map((pin) => pin.event_id)).size !== pins.length ||
    new Set(pins.map((pin) => pin.seq)).size !== pins.length
  ) {
    invalidResponse();
  }
  return pins;
}

function parseListResponse(value: unknown, channelId: string): MessagePin[] {
  const response = strictRecord(value, "메시지 고정 목록");
  assertExactKeys(response, ["pins"], "메시지 고정 목록");
  return parsePins(response.pins, channelId);
}

function parseMutationResponse(
  value: unknown,
  channelId: string,
  expectedEventId: string,
  expectedPinned: boolean
): MessagePin[] {
  const response = strictRecord(value, "메시지 고정 변경");
  assertExactKeys(response, ["pinned", "pins"], "메시지 고정 변경");
  if (response.pinned !== expectedPinned) invalidResponse();
  const pins = parsePins(response.pins, channelId);
  if (pins.some((pin) => pin.event_id === expectedEventId) !== expectedPinned) {
    invalidResponse();
  }
  return pins;
}

async function operationAuthority(
  roomId: string,
  authority: MessagePinsAuthority,
  operation: PinOperation
): Promise<{ baseUrl: string; credential: string; deviceToken?: string }> {
  if (authority.kind === "local") {
    const grant =
      operation === "read"
        ? await requestDesktopMessagePinsReadTicket(roomId)
        : await requestDesktopMessagePinsWriteTicket(roomId);
    return { baseUrl: grant.http_base_url, credential: grant.ticket };
  }
  if (!authority.sessionToken) {
    throw new Error("방 세션 권위를 사용할 수 없습니다.");
  }
  return {
    baseUrl: "",
    credential: authority.sessionToken, deviceToken: authority.deviceToken,
  };
}

function bearer(credential: string, json = false, deviceToken?: string): Headers {
  const headers = new Headers({ Authorization: `Bearer ${credential}` });
  if (deviceToken) headers.set("X-Device-Token", deviceToken);
  if (json) headers.set("Content-Type", "application/json");
  return headers;
}

export async function fetchMessagePins({
  roomId,
  channelId,
  authority,
  beforeDispatch,
}: {
  roomId: string;
  channelId: string;
  authority: MessagePinsAuthority;
  beforeDispatch?: () => void;
}): Promise<MessagePin[]> {
  const canonicalRoom = canonicalRoomId(roomId);
  requireMessageChannelId(channelId);
  const resolved = await operationAuthority(canonicalRoom, authority, "read");
  const path = `/api/room-pins${queryString({
    room_id: canonicalRoom,
    channel_id: channelId,
  })}`;
  beforeDispatch?.();
  const response = await fetch(`${resolved.baseUrl}${path}`, {
    cache: "no-store",
    headers: bearer(resolved.credential, false, resolved.deviceToken),
  });
  if (!response.ok) throw await responseError(response);
  return parseListResponse(await response.json(), channelId);
}

export async function setMessagePinned({
  roomId,
  channelId,
  eventId,
  pinned,
  authority,
  beforeDispatch,
}: {
  roomId: string;
  channelId: string;
  eventId: string;
  pinned: boolean;
  authority: MessagePinsAuthority;
  beforeDispatch?: () => void;
}): Promise<MessagePin[]> {
  const canonicalRoom = canonicalRoomId(roomId);
  requireMessageChannelId(channelId);
  const canonicalEvent = canonicalEventId(eventId);
  const resolved = await operationAuthority(canonicalRoom, authority, "write");
  beforeDispatch?.();
  const response = await fetch(`${resolved.baseUrl}/api/room-pins`, {
    cache: "no-store",
    method: "POST",
    headers: bearer(resolved.credential, true, resolved.deviceToken),
    body: JSON.stringify({
      room_id: canonicalRoom,
      channel_id: channelId,
      event_id: canonicalEvent,
      pinned,
    }),
  });
  if (!response.ok) throw await responseError(response);
  return parseMutationResponse(await response.json(), channelId, canonicalEvent, pinned);
}
