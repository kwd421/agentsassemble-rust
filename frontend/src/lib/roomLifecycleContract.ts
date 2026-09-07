import type { RoomEvent } from "../api";
import type { Room } from "../types/generated/Room";
import { assertExactKeys, strictRecord, type ExactGeneratedKeys } from "./strictJsonContract";

const GENERATED_ROOM_KEYS = ["room_id", "room_uid", "label", "status", "created_at", "updated_at"] as const;
const ROOM_KEYS: ExactGeneratedKeys<Room, typeof GENERATED_ROOM_KEYS> = GENERATED_ROOM_KEYS;

const ROOM_LIFECYCLE_EVENTS = { active: "room_restored", archived: "room_archived", closed: "room_closed" } as const satisfies Record<Room["status"], string>;

export function isRoomLifecycleEvent(event: RoomEvent): boolean {
  return Object.values(ROOM_LIFECYCLE_EVENTS).some((type) => type === event.type);
}

export function parsePublicRoom(value: unknown, expectedRoomId: string): Room {
  const room = strictRecord(value, "방 상태");
  assertExactKeys(room, ROOM_KEYS, "방 상태");
  if (ROOM_KEYS.some((key) => typeof room[key] !== "string") || !room.room_uid ||
      room.room_id !== expectedRoomId || !Object.hasOwn(ROOM_LIFECYCLE_EVENTS, String(room.status))) {
    throw new Error("방 상태가 현재 방과 일치하지 않습니다.");
  }
  return room as Room;
}

export function roomFromLifecycleEvent(event: RoomEvent): Room {
  const room = parsePublicRoom((event as unknown as Record<string, unknown>).room, event.room_id);
  if (room.updated_at !== event.created_at || !isRoomLifecycleEvent(event) ||
      event.type !== ROOM_LIFECYCLE_EVENTS[room.status]) {
    throw new Error("방 상태 이벤트가 일치하지 않습니다.");
  }
  return room;
}
