import type { RoomEvent } from "../api";
import type { Room } from "../types/generated/Room";
import { publicRoomEventIsValid } from "../lib/roomSocketValidation";
import { roomFromLifecycleEvent } from "../lib/roomLifecycleContract";
import { assertExactKeys, strictRecord } from "../lib/strictJsonContract";
import { postJsonServerOperator, responseError, isPrivateNoStoreResponse } from "./http";
import type { RoomHttpAuthority } from "./roomHttpAuthority";

export type RoomLifecycleIntent = {
  serverId: string;
  authorityLineageId: string;
  requestId: string;
  roomId: string;
  roomUid: string;
  action: "room.close" | "room.archive" | "room.delete";
  archived?: boolean;
  confirmationName?: string;
};

export async function changeRoomLifecycle(intent: RoomLifecycleIntent, beforeDispatch: () => void, session?: Extract<RoomHttpAuthority, { kind: "remote" }>): Promise<{ room: Room; cleanupPending: boolean; deleted?: boolean }> {
  const deleting = intent.action === "room.delete";
  const body = {
    server_id: intent.serverId, authority_lineage_id: intent.authorityLineageId,
    room_id: intent.roomId, request_id: intent.requestId, action: intent.action,
    payload: { room_uid: intent.roomUid, ...(intent.action === "room.archive" ? { archived: intent.archived } : {}), ...(deleting ? { confirmation_name: intent.confirmationName } : {}) },
  };
  let raw: unknown;
  if (session) {
    if (!session.sessionToken || !session.deviceToken) throw new Error("현재 기기의 방 세션 권위를 사용할 수 없습니다.");
    beforeDispatch();
    const response = await fetch("/api/room-session/lifecycle", {
      method: "POST", cache: "no-store", redirect: "error",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${session.sessionToken}`, "X-Device-Token": session.deviceToken },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw await responseError(response);
    if (!isPrivateNoStoreResponse(response, "application/json")) throw new Error("방 관리 응답이 올바르지 않습니다.");
    raw = await response.json();
  } else {
    raw = await postJsonServerOperator<unknown>("/api/rooms/lifecycle", body, beforeDispatch);
  }
  const response = strictRecord(raw, "방 관리 응답");
  assertExactKeys(response, ["server_id", "authority_lineage_id", "request_id", "action", "resolution", "result", "deduplicated"], "방 관리 응답");
  const result = strictRecord(response.result, "방 관리 결과");
  assertExactKeys(result, ["room", "event", "event_seq", "events", ...(deleting ? ["deleted"] : ["cleanup_pending", "revoked_sessions"])], "방 관리 결과");
  const event = result.event;
  if (response.server_id !== intent.serverId || response.authority_lineage_id !== intent.authorityLineageId || response.request_id !== intent.requestId || response.action !== intent.action ||
      response.resolution !== "committed" || typeof response.deduplicated !== "boolean" ||
      (deleting ? result.deleted !== true : (typeof result.cleanup_pending !== "boolean" ||
      !Number.isSafeInteger(result.revoked_sessions) || Number(result.revoked_sessions) < 0)) ||
      !Array.isArray(result.events) || result.events.length !== 1 ||
      !publicRoomEventIsValid(result.events[0], intent.roomId) ||
      !publicRoomEventIsValid(event, intent.roomId)) {
    throw new Error("방 관리 응답이 요청과 일치하지 않습니다.");
  }
  const room = roomFromLifecycleEvent(event as RoomEvent);
  const expected = intent.action === "room.close" || deleting ? "closed" : intent.archived ? "archived" : "active";
  const resultRoom = strictRecord(result.room, "변경된 방");
  const publishedRoom = roomFromLifecycleEvent(result.events[0] as RoomEvent);
  assertExactKeys(resultRoom, Object.keys(room), "변경된 방");
  if (result.events[0].id !== event.id || result.events[0].seq !== event.seq ||
      Object.keys(room).some((key) => publishedRoom[key as keyof typeof room] !== room[key as keyof typeof room]) ||
      room.room_uid !== intent.roomUid || room.status !== expected || result.event_seq !== event.seq ||
      Object.keys(room).some((key) => resultRoom[key] !== room[key as keyof typeof room])) {
    throw new Error("변경된 방의 식별자 또는 상태가 일치하지 않습니다.");
  }
  return { room, cleanupPending: !deleting && result.cleanup_pending === true, ...(deleting ? { deleted: true } : {}) };
}
