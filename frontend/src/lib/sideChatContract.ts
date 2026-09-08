import type { SideChatMessage } from "../types/generated/SideChatMessage";
import type { SideChatSnapshot } from "../types/generated/SideChatSnapshot";
import type { SideChatUpdate } from "../types/generated/SideChatUpdate";
import { MAX_TEXT_CHAT_CHARACTERS, SIDE_CHAT_MAX_MESSAGES } from "../types/generated/SIDE_CHAT_WIRE";
import { assertExactKeys, strictRecord } from "./strictJsonContract";
import { isSequence } from "./roomSequence";
import { isUnicodeScalarString } from "./unicodeScalarString";

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

function invalid(): never {
  throw new Error("사이드챗 응답이 현재 방의 기록 계약과 일치하지 않아요.");
}

function lifetime(value: Record<string, unknown>, roomId: string): void {
  if (value.room_id !== roomId || typeof value.generation !== "string" || !UUID_V4.test(value.generation) || !isSequence(value.retained_after_seq)) invalid();
}

function parseMessage(value: unknown): SideChatMessage {
  const message = strictRecord(value, "side chat message");
  assertExactKeys(message, ["id", "seq", "created_at", "participant_id", "display_name", "content"], "side chat message");
  if (
    typeof message.id !== "string" || !UUID_V4.test(message.id) ||
    !isSequence(message.seq) || message.seq === 0 ||
    typeof message.created_at !== "string" || !Number.isFinite(Date.parse(message.created_at)) ||
    typeof message.participant_id !== "string" || !message.participant_id ||
    typeof message.display_name !== "string" || !message.display_name ||
    typeof message.content !== "string" || !message.content || !isUnicodeScalarString(message.content) ||
    [...message.content].length > MAX_TEXT_CHAT_CHARACTERS
  ) invalid();
  return message as unknown as SideChatMessage;
}

export function parseSideChatUpdate(value: unknown, roomId: string): SideChatUpdate {
  const update = strictRecord(value, "side chat update");
  assertExactKeys(update, ["room_id", "generation", "retained_after_seq", "message"], "side chat update");
  lifetime(update, roomId);
  const message = parseMessage(update.message);
  if (message.seq <= Number(update.retained_after_seq)) invalid();
  return { ...(update as unknown as SideChatUpdate), message };
}

export function parseSideChatSnapshot(value: unknown, roomId: string, roomUid: string): SideChatSnapshot {
  const snapshot = strictRecord(value, "side chat snapshot");
  assertExactKeys(snapshot, ["room_id", "room_uid", "generation", "retained_after_seq", "latest_seq", "messages"], "side chat snapshot");
  lifetime(snapshot, roomId);
  if (snapshot.room_uid !== roomUid) invalid();
  if (!isSequence(snapshot.latest_seq) || !Array.isArray(snapshot.messages) || snapshot.messages.length > SIDE_CHAT_MAX_MESSAGES) invalid();
  const messages = snapshot.messages.map(parseMessage);
  const floor = Number(snapshot.retained_after_seq);
  if (floor + messages.length !== snapshot.latest_seq || messages.some((message, index) => message.seq !== floor + index + 1)) invalid();
  return { ...(snapshot as unknown as SideChatSnapshot), messages };
}
