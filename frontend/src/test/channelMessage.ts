import type { RoomEvent } from "../api";
export const channelId = "c0123456789ab";
export function channelMessage(seq: number, channel = channelId): RoomEvent {
  return { v: 1, id: `channel-event-${seq}`, room_id: "general", seq,
    created_at: "2026-09-08T00:00:00Z", type: "channel_message_final", channel_id: channel,
    actor: { participant_id: "operator-local", participant_type: "human" },
    actor_id: "operator-local", participant_id: "operator-local", actor_type: "human", participant_type: "human",
    display_name: "Host", message_kind: "message", content: `message ${seq}`,
  };
}
