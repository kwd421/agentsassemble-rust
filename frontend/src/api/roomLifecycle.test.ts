import { expect, it, vi } from "vitest";
import { postJsonServerOperator } from "./http";
import { changeRoomLifecycle, type RoomLifecycleIntent } from "./roomLifecycle";

vi.mock("./http", () => ({ postJsonServerOperator: vi.fn() }));

it("binds lifecycle confirmation to the authority, incarnation and published room", async () => {
  const intent: RoomLifecycleIntent = { serverId: "server", authorityLineageId: "lineage", requestId: "request", roomId: "general", roomUid: "incarnation", action: "room.archive", archived: true };
  const room = { room_id: "general", room_uid: "incarnation", label: "General", status: "archived", created_at: "2026-09-07T00:00:00Z", updated_at: "2026-09-07T00:00:01Z" };
  const event = { id: "event", v: 1, room_id: "general", seq: 1, created_at: room.updated_at, type: "room_archived", actor: { participant_id: "operator-local", participant_type: "human" }, room };
  const response = { server_id: "server", authority_lineage_id: "lineage", request_id: "request", action: "room.archive", resolution: "committed", deduplicated: true,
    result: { room, cleanup_pending: false, revoked_sessions: 0, event, event_seq: 1, events: [event] } };
  vi.mocked(postJsonServerOperator).mockResolvedValueOnce(response);
  await expect(changeRoomLifecycle(intent, vi.fn())).resolves.toEqual({ room, cleanupPending: false });
  for (const mutate of [
    (copy: typeof response) => { copy.authority_lineage_id = "replacement"; },
    (copy: typeof response) => { copy.result.room.room_uid = "replacement"; },
    (copy: typeof response) => { copy.result.events[0].room.label = "Different"; },
    (copy: typeof response) => { copy.result.event_seq = 2; },
  ]) {
    const copy = JSON.parse(JSON.stringify(response)) as typeof response;
    mutate(copy);
    vi.mocked(postJsonServerOperator).mockResolvedValueOnce(copy);
    await expect(changeRoomLifecycle(intent, vi.fn())).rejects.toThrow();
  }
});
