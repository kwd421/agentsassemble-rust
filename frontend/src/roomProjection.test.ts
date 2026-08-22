import { describe, expect, it } from "vitest";
import type { RoomEvent } from "./api";
import { acceptSnapshotEvents } from "./roomProjection";

function event(id: string, seq: number): RoomEvent {
  return {
    v: 1,
    id,
    seq,
    created_at: "2026-08-22T00:00:00Z",
    room_id: "general",
    type: "message_final",
    actor: { participant_id: "operator-local", participant_type: "human" },
    content: id,
  };
}

describe("room snapshot projection", () => {
  it("keeps the existing projection when an empty resume confirms its cursor", () => {
    expect(acceptSnapshotEvents([event("one", 1)], [], "resume")).toEqual([event("one", 1)]);
  });

  it("replaces stale state for an authoritative initial snapshot", () => {
    expect(acceptSnapshotEvents([event("stale", 1)], [event("fresh", 2)], "initial"))
      .toEqual([event("fresh", 2)]);
  });
});
