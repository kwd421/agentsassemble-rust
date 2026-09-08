import type { SideChatSnapshot } from "../types/generated/SideChatSnapshot";
import type { SideChatUpdate } from "../types/generated/SideChatUpdate";
import { SIDE_CHAT_MAX_MESSAGES } from "../types/generated/SIDE_CHAT_WIRE";

/** Merge the HTTP cut with its overlapping live delivery, without advancing room history. */
export function mergeSideChatUpdates(snapshot: SideChatSnapshot, updates: readonly SideChatUpdate[]): SideChatSnapshot {
  let floor = snapshot.retained_after_seq;
  let latest = snapshot.latest_seq;
  const messages = new Map(snapshot.messages.map((message) => [message.seq, message]));
  for (const update of updates) {
    if (update.room_id !== snapshot.room_id || update.generation !== snapshot.generation) {
      throw new Error("사이드챗 기록이 새로 시작됐어요. 다시 연결해 주세요.");
    }
    floor = Math.max(floor, update.retained_after_seq);
    latest = Math.max(latest, update.message.seq);
    const previous = messages.get(update.message.seq);
    if (previous && (["id", "created_at", "participant_id", "display_name", "content"] as const).some((field) => previous[field] !== update.message[field])) {
      throw new Error("사이드챗 메시지가 기존 기록과 일치하지 않아요.");
    }
    if (update.message.seq > floor) messages.set(update.message.seq, update.message);
  }
  const retained = [...messages.values()].filter((message) => message.seq > floor).sort((a, b) => a.seq - b.seq);
  if (retained.length > SIDE_CHAT_MAX_MESSAGES || floor + retained.length !== latest || retained.some((message, index) => message.seq !== floor + index + 1)) {
    throw new Error("사이드챗 기록 일부를 받지 못했어요. 다시 연결해 주세요.");
  }
  return { ...snapshot, retained_after_seq: floor, latest_seq: latest, messages: retained };
}
