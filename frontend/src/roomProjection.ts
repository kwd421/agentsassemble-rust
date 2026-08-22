import type { RoomEvent } from "./api";
import type { SnapshotMode } from "./types/generated/SnapshotMode";

export function mergeRoomEvents(current: RoomEvent[], incoming: RoomEvent[]): RoomEvent[] {
  const byId = new Map(current.map((event) => [event.id, event]));
  for (const event of incoming) byId.set(event.id, event);
  return [...byId.values()].sort((left, right) => left.seq - right.seq);
}

export function acceptSnapshotEvents(
  current: RoomEvent[],
  incoming: RoomEvent[],
  mode: SnapshotMode
): RoomEvent[] {
  return mode === "resume" ? mergeRoomEvents(current, incoming) : incoming;
}
