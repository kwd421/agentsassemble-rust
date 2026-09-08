import { requestDesktopSideChatReadTicket } from "../lib/desktopBridge";
import { canonicalRoomId } from "../lib/canonicalRoomId";
import { parseSideChatSnapshot } from "../lib/sideChatContract";
import { isPrivateNoStoreResponse, queryString, responseError } from "./http";
import type { RoomHttpAuthority } from "./roomHttpAuthority";

export async function fetchSideChatSnapshot(
  roomId: string,
  roomUid: string,
  authority: RoomHttpAuthority,
  signal: AbortSignal,
) {
  const room = canonicalRoomId(roomId);
  const grant = authority.kind === "local"
    ? await requestDesktopSideChatReadTicket(room)
    : { http_base_url: "", ticket: authority.sessionToken };
  signal.throwIfAborted();
  if (!grant.ticket) throw new Error("사이드챗 읽기 권한을 사용할 수 없어요.");
  const response = await fetch(`${grant.http_base_url}/api/side-chat${queryString({ room_id: room })}`, {
    signal, cache: "no-store",
    headers: {
      Authorization: `Bearer ${grant.ticket}`,
      ...(authority.kind === "remote" && authority.deviceToken ? { "X-Device-Token": authority.deviceToken } : {}),
    },
  });
  if (!response.ok) throw await responseError(response);
  if (!isPrivateNoStoreResponse(response, "application/json")) throw new Error("사이드챗 응답의 보호 설정이 올바르지 않아요.");
  const result = parseSideChatSnapshot(await response.json(), room, roomUid);
  signal.throwIfAborted();
  return result;
}
