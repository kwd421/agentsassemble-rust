import type { RoomAgentSession, RoomEvent } from "./api";
import { agentSessionIsValid } from "./roomSocketSchema";
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

export function projectAgentSessionEvents(
  current: RoomAgentSession[],
  incoming: RoomEvent[]
): RoomAgentSession[] {
  const byId = new Map(current.map((session) => [session.session_id, session]));
  for (const event of incoming) {
    const session = event.type === "agent_session_state" ? event.agent_session : null;
    if (agentSessionIsValid(session, event.room_id)) {
      const projected = session as RoomAgentSession;
      byId.set(projected.session_id, projected);
    }
  }
  return [...byId.values()].sort((left, right) => left.session_id.localeCompare(right.session_id));
}

export function visibleTimelineEvents(events: RoomEvent[]): RoomEvent[] {
  return events.filter((event) => event.type === "message_final");
}
