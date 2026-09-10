import { requestDesktopAttendeeInviteCreateTicket, type DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import { assertExactKeys, requiredString, strictRecord } from "../lib/strictJsonContract";
import { parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import type { AttendeeEntryPacket } from "../types/generated/AttendeeEntryPacket";
import type { CreateCompanionAttendeeInvite } from "../types/generated/CreateCompanionAttendeeInvite";
import type { CreateFriendAttendeeInvite } from "../types/generated/CreateFriendAttendeeInvite";

export type AttendeePacketCustody = { result: AttendeeEntryPacket; origin: string; expiresAtMs: number };
type Expected = { requestId: string; roomId: string; roomUid: string };

export async function createFriendAttendeeInvite(
  authority: DesktopManagerRoomAuthority, request: CreateFriendAttendeeInvite, assertCurrent: () => void,
): Promise<AttendeePacketCustody> {
  const ticket = await requestDesktopAttendeeInviteCreateTicket(authority);
  assertCurrent();
  const response = await fetch(`${ticket.http_base_url}/api/room-attendee/friend-invite`, {
    method: "POST", cache: "no-store", redirect: "error",
    headers: { Authorization: `Bearer ${ticket.ticket}`, "Content-Type": "application/json" }, body: JSON.stringify(request),
  });
  return readPacket(response, { requestId: request.request_id, roomId: authority.room_id, roomUid: authority.room_uid });
}

export async function createCompanionAttendeeInvite(
  session: Pick<RoomGuestSession, "roomUid" | "sessionToken" | "meetingId">, request: CreateCompanionAttendeeInvite,
): Promise<AttendeePacketCustody> {
  if (!session.roomUid || !session.sessionToken) throw new Error("현재 방의 참가 권한을 확인할 수 없어요.");
  const response = await fetch("/api/room-attendee/companion-invite", {
    method: "POST", cache: "no-store", redirect: "error",
    headers: { Authorization: `Bearer ${session.sessionToken}`, "Content-Type": "application/json" }, body: JSON.stringify(request),
  });
  const packet = await readPacket(response, { requestId: request.request_id, roomId: session.meetingId, roomUid: session.roomUid });
  if (packet.origin !== window.location.origin) throw new Error("공개 주소가 변경됐어요. 현재 방 주소에서 다시 시도해 주세요.");
  return packet;
}

async function readPacket(response: Response, expected: Expected): Promise<AttendeePacketCustody> {
  if (!response.ok) {
    const failure = await response.json().catch(() => null) as { error?: { code?: string } } | null;
    const message = failure?.error?.code === "unsupported_provider" ? "이 제공자는 AI 참가자로 연결할 수 없어요. 제공자 이름을 확인해 주세요."
      : failure?.error?.code === "companion_limit_reached" ? "동반 AI 초대와 참가자는 최대 8명까지 유지할 수 있어요."
      : failure?.error?.code === "public_ingress_not_ready" ? "공개 연결을 시작한 뒤 초대를 만들어 주세요."
      : "AI 참가 초대 결과를 확인하지 못했어요. 현재 권한을 확인하고 다시 시도해 주세요.";
    throw new Error(message);
  }
  return parseAttendeeEntryPacket(await response.json(), expected);
}

export function parseAttendeeEntryPacket(input: unknown, expected: Expected): AttendeePacketCustody {
  const value = strictRecord(input, "AI 참가 초대");
  const keys = ["request_id", "room_id", "room_uid", "invite_id", "expires_at", "display_name", "provider", "attend_command", "join_url"] as const;
  assertExactKeys(value, keys, "AI 참가 초대");
  const result = Object.fromEntries(keys.map((key) => [key, requiredString(value, key, "AI 참가 초대")])) as AttendeeEntryPacket;
  const url = new URL(result.join_url);
  parsePublicIngressOrigin(url.origin);
  const expiresAtMs = Date.parse(result.expires_at);
  if (result.request_id !== expected.requestId || result.room_id !== expected.roomId || result.room_uid !== expected.roomUid ||
      !Number.isFinite(expiresAtMs) || !/^[a-z][a-z0-9_]{0,63}$/.test(result.provider) ||
      result.attend_command !== `assemble room attend --provider ${result.provider}` ||
      url.pathname !== "/join" || url.username || url.password || url.hash ||
      [...url.searchParams.keys()].length !== 1 || !url.searchParams.get("token")) {
    throw new Error("AI 참가 초대 응답이 현재 방과 일치하지 않아요. 다시 시도해 주세요.");
  }
  return { result, origin: url.origin, expiresAtMs };
}

export function attendeePacketText(packet: AttendeeEntryPacket): string {
  return `${packet.attend_command}\n\n위 명령을 실행한 뒤 숨김 입력창에 아래 초대 URL을 붙여 넣어 주세요.\n${packet.join_url}\n\n한 번 사용할 수 있어요. 만료: ${new Date(packet.expires_at).toLocaleString()}`;
}
