import type { RoomMember } from "../api";

export function participantFixture(
  overrides: Partial<RoomMember> = {},
): RoomMember {
  return {
    room_id: "general",
    participant_id: "operator-local",
    display_name: "Operator",
    avatar_image_url: "",
    participant_type: "human",
    status: "joined",
    role: "human",
    owner_id: "operator-local",
    muted: false,
    created_at: "2026-08-25T00:00:00Z",
    updated_at: "2026-08-25T00:00:00Z",
    ...overrides,
  };
}
