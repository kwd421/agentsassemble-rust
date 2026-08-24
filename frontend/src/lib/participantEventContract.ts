import type { RoomEvent, RoomMember } from "../api";

const PARTICIPANT_KEYS = [
  "room_id",
  "participant_id",
  "display_name",
  "avatar_image_url",
  "participant_type",
  "status",
  "role",
  "owner_id",
  "muted",
  "created_at",
  "updated_at",
] as const;

export function joinedParticipantFromEvent(event: RoomEvent): RoomMember {
  const value = (event as unknown as Record<string, unknown>).participant;
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("participant_joined 이벤트에 참가자 투영이 없습니다.");
  }
  const participant = value as Record<string, unknown>;
  const actualKeys = Object.keys(participant).sort();
  const expectedKeys = [...PARTICIPANT_KEYS].sort();
  if (
    actualKeys.length !== expectedKeys.length ||
    actualKeys.some((key, index) => key !== expectedKeys[index]) ||
    PARTICIPANT_KEYS.filter((key) => key !== "muted").some(
      (key) => typeof participant[key] !== "string"
    ) ||
    typeof participant.muted !== "boolean" ||
    !participant.participant_id ||
    participant.participant_id !== event.participant_id ||
    participant.room_id !== event.room_id ||
    participant.status !== "joined"
  ) {
    throw new Error("participant_joined 이벤트의 참가자 투영이 올바르지 않습니다.");
  }
  return {
    meeting_id: participant.room_id as string,
    participant_id: participant.participant_id as string,
    display_name: participant.display_name as string,
    avatar_image_url: participant.avatar_image_url as string,
    role: participant.role as RoomMember["role"],
    participant_type: participant.participant_type as RoomMember["participant_type"],
    provider_kind: "",
    connection_kind: "",
    owner_id: participant.owner_id as string,
    status: participant.status as string,
    muted: participant.muted as boolean,
    source: participant.participant_type === "human" ? "room" : "agent_session",
    created_at: participant.created_at as string,
    updated_at: participant.updated_at as string,
  };
}
