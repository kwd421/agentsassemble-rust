import type { RoomEvent } from "../api";
import type { Room } from "../types/generated/Room";
import { assertExactKeys, strictRecord, type ExactGeneratedKeys } from "./strictJsonContract";

const GENERATED_ROOM_KEYS = ["room_id", "room_uid", "label", "status", "created_at", "updated_at"] as const;
const ROOM_KEYS: ExactGeneratedKeys<Room, typeof GENERATED_ROOM_KEYS> = GENERATED_ROOM_KEYS;

export function isRoomLifecycleEvent(event: RoomEvent): boolean {
  return ["room_closed", "room_archived", "room_restored"].includes(event.type);
}

export function roomFromLifecycleEvent(event: RoomEvent): Room {
  const room = strictRecord((event as unknown as Record<string, unknown>).room, "방 상태");
  assertExactKeys(room, ROOM_KEYS, "방 상태");
  if (ROOM_KEYS.some((key) => typeof room[key] !== "string") || !room.room_uid ||
      room.room_id !== event.room_id || room.updated_at !== event.created_at ||
      !isRoomLifecycleEvent(event) ||
      event.type !== ({ active: "room_restored", archived: "room_archived", closed: "room_closed" } as Record<string, string>)[String(room.status)]) {
    throw new Error("방 상태 이벤트가 일치하지 않습니다.");
  }
  return room as Room;
}
