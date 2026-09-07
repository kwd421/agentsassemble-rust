import { afterEach, expect, it, vi } from "vitest";
import { postJsonServerOperator } from "./http";
import { changeRoomLifecycle, type RoomLifecycleIntent } from "./roomLifecycle";

vi.mock("./http", async () => ({ ...await vi.importActual<typeof import("./http")>("./http"), postJsonServerOperator: vi.fn() }));
afterEach(() => vi.unstubAllGlobals());

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
  const deletedRoom = { ...room, status: "closed" };
  const terminal = { ...event, type: "room_closed", room: deletedRoom };
  const deletion = { ...intent, action: "room.delete" as const, confirmationName: "General" };
  const completed = { ...response, action: "room.delete", result: { room: deletedRoom, deleted: true, event: terminal, event_seq: terminal.seq, events: [terminal] } };
  vi.mocked(postJsonServerOperator).mockResolvedValueOnce(completed);
  await expect(changeRoomLifecycle(deletion, vi.fn())).resolves.toEqual({ room: deletedRoom, cleanupPending: false, deleted: true });
  vi.mocked(postJsonServerOperator).mockResolvedValueOnce({ ...completed, result: { ...completed.result, deleted: false } });
  await expect(changeRoomLifecycle(deletion, vi.fn())).rejects.toThrow();
});

it("uses the exact paired bearer/device and preserves a pending deletion response", async () => {
  vi.mocked(postJsonServerOperator).mockClear();
  const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
    code: "room_deletion_pending", error: "Deletion accepted", resolution: "unresolved",
  }), { status: 503, headers: { "Content-Type": "application/json", "Cache-Control": "private, no-store" } }));
  vi.stubGlobal("fetch", fetchMock);
  const beforeDispatch = vi.fn();
  await expect(changeRoomLifecycle({ serverId: "server", authorityLineageId: "lineage", requestId: "same-request", roomId: "general", roomUid: "exact", action: "room.delete", confirmationName: "General" },
    beforeDispatch, { kind: "remote", sessionToken: "aops1.paired", deviceToken: "device" })).rejects.toMatchObject({
      code: "room_deletion_pending", resolution: "unresolved",
    });
  expect(beforeDispatch).toHaveBeenCalledOnce();
  expect(postJsonServerOperator).not.toHaveBeenCalled();
  expect(fetchMock).toHaveBeenCalledWith("/api/room-session/lifecycle", expect.objectContaining({
    redirect: "error", cache: "no-store", method: "POST",
    headers: { "Content-Type": "application/json", Authorization: "Bearer aops1.paired", "X-Device-Token": "device" },
  }));
});
