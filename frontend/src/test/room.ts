import type { Room } from "../types/generated/Room";

export function roomFixture(overrides: Partial<Room> = {}): Room {
  return { room_id: "general", room_uid: "a54e2293-3121-4f53-8b34-20aa747d6b34", label: "General", status: "active",
    created_at: "2026-09-07T00:00:00Z", updated_at: "2026-09-07T00:00:00Z", ...overrides };
}
