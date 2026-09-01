import { requestDesktopMessageSearchReadTicket } from "../lib/desktopBridge";
import { canonicalRoomId } from "../lib/canonicalRoomId";
import { assertExactKeys, strictRecord } from "../lib/strictJsonContract";
import { isUnicodeScalarString } from "../lib/unicodeScalarString";
import type { RoomEvent } from "../types/generatedRoomEvent";
import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";
import { MAX_MESSAGE_EVENT_ID_BYTES } from "../types/generated/MESSAGE_PINS_WIRE";
import {
  MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS,
  MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS,
  MAX_MESSAGE_SEARCH_CURSOR_BYTES,
  MESSAGE_CONTEXT_RADIUS,
  MESSAGE_SEARCH_PAGE_SIZE,
} from "../types/generated/MESSAGE_SEARCH_WIRE";
import {
  MAX_VOTE_DURATION_SECONDS,
  MAX_VOTE_OPTIONS,
  MIN_VOTE_DURATION_SECONDS,
  MIN_VOTE_OPTIONS,
  VOTE_OPTION_CHARACTER_LIMIT,
  VOTE_QUESTION_CHARACTER_LIMIT,
} from "../types/generated/VOTE_WIRE";
import {
  parseMessageAttachment,
  parseMessageAttachmentFilename,
} from "./messageAttachments";
import {
  exchangeSessionHttpTicket,
  isPrivateNoStoreResponse,
  queryString,
  responseError,
} from "./http";
import type { RoomHttpAuthority } from "./roomHttpAuthority";

export type MessageSearchAuthority = RoomHttpAuthority;

export type RoomSearchResult = Readonly<{
  event_id: string;
  participant_id: string;
  channel_id: "lobby";
  seq: number;
  created_at: string;
  author: string;
  content: string;
  attachment_filenames: string[];
}>;

export type RoomSearchPage = Readonly<{
  results: RoomSearchResult[];
  next_cursor: string;
}>;

export type RoomMessageContext = Readonly<{
  channel_id: "lobby";
  event_id: string;
  events: RoomEvent[];
}>;

type SearchGrant = Readonly<{ baseUrl: string; ticket: string }>;

const SEARCH_RESULT_KEYS = [
  "event_id",
  "participant_id",
  "channel_id",
  "seq",
  "created_at",
  "author",
  "content",
  "attachment_filenames",
] as const;
const CONTEXT_EVENT_KEYS = [
  "v",
  "id",
  "seq",
  "created_at",
  "room_id",
  "type",
  "actor",
  "participant_id",
  "participant_type",
  "actor_id",
  "actor_type",
  "display_name",
  "content",
  "message_kind",
] as const;
const ROOM_PORTAL_KEYS = [
  "session_id",
  "turn_id",
  "source_event_id",
  "target_agent_id",
  "message_source",
] as const;
const ROOM_TOOL_KEYS = [
  "message_source",
  "room_result_id",
  "room_result_kind",
  "operation",
  "source_turn_id",
  "source_participant_id",
  "details",
] as const;
const VOTE_EVENT_KEYS = [
  "vote_question",
  "vote_options",
  "vote_duration_seconds",
  "vote_deadline_at",
] as const;
const RFC3339_UTC = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?Z$/;
const RFC3339_UTC_OFFSET =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?\+00:00$/;
const CURSOR = /^[A-Za-z0-9_-]+$/;

function invalidResponse(): never {
  throw new Error("로비 메시지 검색 응답 계약이 올바르지 않습니다.");
}

function boundedString(value: unknown, limit: number, allowEmpty = false): string {
  if (
    typeof value !== "string" ||
    !isUnicodeScalarString(value) ||
    (!allowEmpty && !value) ||
    [...value].length > limit ||
    value.includes("\0")
  ) {
    invalidResponse();
  }
  return value;
}

function eventId(value: unknown): string {
  const result = boundedString(value, MAX_MESSAGE_EVENT_ID_BYTES);
  if (new TextEncoder().encode(result).byteLength > MAX_MESSAGE_EVENT_ID_BYTES) {
    invalidResponse();
  }
  return result;
}

function timestamp(value: unknown): string {
  const result = boundedString(value, 40);
  if (!RFC3339_UTC.test(result) || Number.isNaN(Date.parse(result))) invalidResponse();
  return result;
}

function voteDeadlineTimestamp(value: unknown): string {
  const result = boundedString(value, 40);
  if (!RFC3339_UTC_OFFSET.test(result) || Number.isNaN(Date.parse(result))) invalidResponse();
  return result;
}

function timestampNanosKey(value: string): string {
  const body = value.slice(0, -1);
  const separator = body.indexOf(".");
  if (separator < 0) return `${body}.000000000`;
  return `${body.slice(0, separator)}.${body.slice(separator + 1).padEnd(9, "0")}`;
}

function positiveSequence(value: unknown): number {
  if (!Number.isSafeInteger(value) || Number(value) < 1) invalidResponse();
  return Number(value);
}

function hasVisibleText(value: string): boolean {
  return [...value].some(
    (character) => !/[\p{White_Space}\p{Cc}\p{Cf}]/u.test(character)
  );
}

function parseSearchResult(value: unknown): RoomSearchResult {
  const result = strictRecord(value, "로비 메시지 검색 결과");
  assertExactKeys(result, SEARCH_RESULT_KEYS, "로비 메시지 검색 결과");
  const content = boundedString(
    result.content,
    MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS,
    true
  );
  if (
    result.channel_id !== "lobby" ||
    !Array.isArray(result.attachment_filenames) ||
    result.attachment_filenames.length > MAX_MESSAGE_ATTACHMENTS_PER_EVENT
  ) {
    invalidResponse();
  }
  const filenames = result.attachment_filenames.map(parseMessageAttachmentFilename);
  if (!hasVisibleText(content) && filenames.length === 0) invalidResponse();
  return Object.freeze({
    event_id: eventId(result.event_id),
    participant_id: boundedString(result.participant_id, 256),
    channel_id: "lobby",
    seq: positiveSequence(result.seq),
    created_at: timestamp(result.created_at),
    author: boundedString(result.author, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS),
    content,
    attachment_filenames: filenames,
  });
}

function parseSearchPage(value: unknown): RoomSearchPage {
  const page = strictRecord(value, "로비 메시지 검색 페이지");
  assertExactKeys(page, ["results", "next_cursor"], "로비 메시지 검색 페이지");
  if (!Array.isArray(page.results) || page.results.length > MESSAGE_SEARCH_PAGE_SIZE) {
    invalidResponse();
  }
  const results = page.results.map(parseSearchResult);
  const cursor = boundedString(page.next_cursor, MAX_MESSAGE_SEARCH_CURSOR_BYTES, true);
  if (
    (cursor && (!CURSOR.test(cursor) || results.length !== MESSAGE_SEARCH_PAGE_SIZE)) ||
    new TextEncoder().encode(cursor).byteLength > MAX_MESSAGE_SEARCH_CURSOR_BYTES ||
    new Set(results.map((result) => result.event_id)).size !== results.length ||
    new Set(results.map((result) => result.seq)).size !== results.length ||
    results.some((result, index) => {
      if (index === 0) return false;
      const previous = results[index - 1];
      const previousTime = timestampNanosKey(previous.created_at);
      const currentTime = timestampNanosKey(result.created_at);
      return previousTime < currentTime ||
        (previousTime === currentTime && previous.seq <= result.seq);
    })
  ) {
    invalidResponse();
  }
  return Object.freeze({
    results,
    next_cursor: cursor,
  });
}

function parseActor(value: unknown): Readonly<{ participant_id: string; participant_type: string }> {
  const actor = strictRecord(value, "로비 메시지 검색 actor");
  assertExactKeys(actor, ["participant_id", "participant_type"], "로비 메시지 검색 actor");
  return Object.freeze({
    participant_id: boundedString(actor.participant_id, 256),
    participant_type: boundedString(actor.participant_type, 64),
  });
}

function parseRoomToolDetails(event: Record<string, unknown>): Readonly<Record<string, unknown>> {
  const details = strictRecord(event.details, "방 도구 검색 컨텍스트");
  if (event.operation === "roll_dice" && event.room_result_kind === "dice_roll") {
    assertExactKeys(details, ["notation", "rolls", "modifier", "total"], "방 도구 검색 컨텍스트");
    if (
      typeof details.notation !== "string" ||
      !/^\d{1,3}d\d{1,4}(?:[+-]\d{1,6})?$/.test(details.notation) ||
      !Array.isArray(details.rolls) ||
      !(1 <= details.rolls.length && details.rolls.length <= 100) ||
      details.rolls.some((roll) => !Number.isSafeInteger(roll) || Number(roll) < 1 || Number(roll) > 1000) ||
      !Number.isSafeInteger(details.modifier) ||
      Math.abs(Number(details.modifier)) > 100_000 ||
      !Number.isSafeInteger(details.total) ||
      details.rolls.reduce((sum, roll) => sum + Number(roll), Number(details.modifier)) !== details.total
    ) {
      invalidResponse();
    }
  } else if (event.operation === "choose_random" && event.room_result_kind === "random_choice") {
    assertExactKeys(details, ["choice", "index", "option_count"], "방 도구 검색 컨텍스트");
    if (
      typeof details.choice !== "string" ||
      !hasVisibleText(boundedString(details.choice, 200)) ||
      !Number.isSafeInteger(details.index) ||
      Number(details.index) < 0 ||
      !Number.isSafeInteger(details.option_count) ||
      Number(details.option_count) < 2 ||
      Number(details.option_count) > 50 ||
      Number(details.index) >= Number(details.option_count)
    ) {
      invalidResponse();
    }
  } else {
    invalidResponse();
  }
  return Object.freeze({ ...details });
}

function parseVoteFields(event: Record<string, unknown>, createdAt: string) {
  const question = boundedString(event.vote_question, VOTE_QUESTION_CHARACTER_LIMIT);
  if (
    !hasVisibleText(question) ||
    !Array.isArray(event.vote_options) ||
    event.vote_options.length < MIN_VOTE_OPTIONS ||
    event.vote_options.length > MAX_VOTE_OPTIONS
  ) {
    invalidResponse();
  }
  const options = event.vote_options.map((option) => {
    const parsed = boundedString(option, VOTE_OPTION_CHARACTER_LIMIT);
    if (!hasVisibleText(parsed)) invalidResponse();
    return parsed;
  });
  const duration = event.vote_duration_seconds;
  if (
    new Set(options).size !== options.length ||
    !Number.isSafeInteger(duration) ||
    !(
      duration === 0 ||
      (Number(duration) >= MIN_VOTE_DURATION_SECONDS &&
        Number(duration) <= MAX_VOTE_DURATION_SECONDS)
    )
  ) {
    invalidResponse();
  }
  const deadline = boundedString(event.vote_deadline_at, 40, true);
  if (
    (duration === 0 && deadline !== "") ||
    (duration !== 0 &&
      (deadline === "" ||
        voteDeadlineTimestamp(deadline) !== deadline ||
        Date.parse(deadline) - Date.parse(createdAt) !== Number(duration) * 1000))
  ) {
    invalidResponse();
  }
  return Object.freeze({
    vote_question: question,
    vote_options: options,
    vote_duration_seconds: Number(duration),
    vote_deadline_at: deadline,
  });
}

function parseContextEvent(value: unknown, roomId: string): RoomEvent {
  const event = strictRecord(value, "로비 메시지 검색 컨텍스트 이벤트");
  const source = event.message_source;
  if (source === undefined) {
    assertExactKeys(
      event,
      event.message_kind === "vote"
        ? [...CONTEXT_EVENT_KEYS, ...VOTE_EVENT_KEYS]
        : CONTEXT_EVENT_KEYS,
      "로비 메시지 검색 컨텍스트 이벤트",
      ["attachments"]
    );
  } else if (source === "room_portal") {
    assertExactKeys(event, [...CONTEXT_EVENT_KEYS, ...ROOM_PORTAL_KEYS], "로비 메시지 검색 컨텍스트 이벤트");
  } else if (source === "room_tool_result") {
    assertExactKeys(event, [...CONTEXT_EVENT_KEYS, ...ROOM_TOOL_KEYS], "로비 메시지 검색 컨텍스트 이벤트");
  } else {
    invalidResponse();
  }
  const actor = parseActor(event.actor);
  const content = boundedString(event.content, MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS, true);
  const attachments = event.attachments === undefined
    ? []
    : Array.isArray(event.attachments) && event.attachments.length <= MAX_MESSAGE_ATTACHMENTS_PER_EVENT
      ? event.attachments.map(parseMessageAttachment)
      : invalidResponse();
  const participantId = boundedString(event.participant_id, 256);
  const participantType = boundedString(event.participant_type, 64);
  const createdAt = timestamp(event.created_at);
  if (
    event.v !== 1 ||
    event.room_id !== roomId ||
    event.type !== "message_final" ||
    event.actor_id !== participantId ||
    event.actor_type !== participantType ||
    actor.participant_id !== participantId ||
    actor.participant_type !== participantType
  ) {
    invalidResponse();
  }
  const messageKind = boundedString(event.message_kind, 32);
  let voteFields: ReturnType<typeof parseVoteFields> | undefined;
  if (source === undefined) {
    if (messageKind === "vote") {
      if (content !== "") invalidResponse();
      voteFields = parseVoteFields(event, createdAt);
    } else if (messageKind !== "message" || (!hasVisibleText(content) && attachments.length === 0)) {
      invalidResponse();
    }
  }
  if (source === "room_portal") {
    if (
      participantType !== "agent" ||
      messageKind !== "message" ||
      attachments.length ||
      !hasVisibleText(content)
    ) invalidResponse();
    ["session_id", "turn_id", "source_event_id"].forEach((key) =>
      boundedString(event[key], 256)
    );
    boundedString(event.target_agent_id, 256, true);
  }
  let details: Readonly<Record<string, unknown>> | undefined;
  if (source === "room_tool_result") {
    if (
      participantId !== "room-system" ||
      participantType !== "system" ||
      messageKind !== "system" ||
      attachments.length ||
      !hasVisibleText(content) ||
      !/^result-[0-9a-f]{32}$/.test(String(event.room_result_id))
    ) {
      invalidResponse();
    }
    boundedString(event.source_turn_id, 256, true);
    boundedString(event.source_participant_id, 256);
    details = parseRoomToolDetails(event);
  }
  return Object.freeze({
    ...event,
    id: eventId(event.id),
    seq: positiveSequence(event.seq),
    created_at: createdAt,
    room_id: roomId,
    type: "message_final",
    actor,
    participant_id: participantId,
    participant_type: participantType,
    actor_id: participantId,
    actor_type: participantType,
    display_name: boundedString(event.display_name, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS),
    content,
    message_kind: messageKind,
    ...(event.attachments !== undefined ? { attachments } : {}),
    ...(voteFields || {}),
    ...(details ? { details } : {}),
  }) as unknown as RoomEvent;
}

function parseContext(value: unknown, roomId: string, expectedEventId: string): RoomMessageContext {
  const context = strictRecord(value, "로비 메시지 검색 컨텍스트");
  assertExactKeys(context, ["channel_id", "event_id", "events"], "로비 메시지 검색 컨텍스트");
  if (
    context.channel_id !== "lobby" ||
    context.event_id !== expectedEventId ||
    !Array.isArray(context.events) ||
    context.events.length === 0 ||
    context.events.length > MESSAGE_CONTEXT_RADIUS * 2 + 1
  ) {
    invalidResponse();
  }
  const events = context.events.map((event) => parseContextEvent(event, roomId));
  const targetIndex = events.findIndex((event) => event.id === expectedEventId);
  if (
    events.filter((event) => event.id === expectedEventId).length !== 1 ||
    new Set(events.map((event) => event.id)).size !== events.length ||
    events.some((event, index) => index > 0 && event.seq <= events[index - 1].seq) ||
    targetIndex > MESSAGE_CONTEXT_RADIUS ||
    events.length - targetIndex - 1 > MESSAGE_CONTEXT_RADIUS
  ) {
    invalidResponse();
  }
  return Object.freeze({
    channel_id: "lobby",
    event_id: expectedEventId,
    events,
  });
}

async function searchGrant(roomId: string, authority: MessageSearchAuthority): Promise<SearchGrant> {
  if (authority.kind === "local") {
    const grant = await requestDesktopMessageSearchReadTicket(roomId);
    return { baseUrl: grant.http_base_url, ticket: grant.ticket };
  }
  if (!authority.sessionToken) throw new Error("방 세션 권위를 사용할 수 없습니다.");
  return {
    baseUrl: "",
    ticket: await exchangeSessionHttpTicket("message-search-read", authority.sessionToken),
  };
}

async function fetchSearchJson(
  path: string,
  grant: SearchGrant,
  beforeDispatch?: () => void
): Promise<unknown> {
  beforeDispatch?.();
  const response = await fetch(`${grant.baseUrl}${path}`, {
    cache: "no-store",
    headers: { Authorization: `Bearer ${grant.ticket}` },
  });
  if (!response.ok) throw await responseError(response);
  if (!isPrivateNoStoreResponse(response, "application/json")) invalidResponse();
  return response.json();
}

export async function searchRoomMessages({
  roomId,
  channelId,
  query,
  authority,
  cursor = "",
  beforeDispatch,
}: {
  roomId: string;
  channelId: string;
  query: string;
  authority: MessageSearchAuthority;
  cursor?: string;
  beforeDispatch?: () => void;
}): Promise<RoomSearchPage> {
  const room = canonicalRoomId(roomId);
  const grant = await searchGrant(room, authority);
  const path = `/api/room-search${queryString({
    room_id: room,
    channel_id: channelId,
    q: query,
    cursor: cursor || undefined,
  })}`;
  return parseSearchPage(await fetchSearchJson(path, grant, beforeDispatch));
}

export async function fetchRoomMessageContext({
  roomId,
  channelId,
  eventId: rawEventId,
  authority,
  beforeDispatch,
}: {
  roomId: string;
  channelId: string;
  eventId: string;
  authority: MessageSearchAuthority;
  beforeDispatch?: () => void;
}): Promise<RoomMessageContext> {
  const room = canonicalRoomId(roomId);
  const target = eventId(rawEventId);
  const grant = await searchGrant(room, authority);
  const path = `/api/room-search/context${queryString({
    room_id: room,
    channel_id: channelId,
    event_id: target,
  })}`;
  return parseContext(await fetchSearchJson(path, grant, beforeDispatch), room, target);
}
