import { requestDesktopConnectorInviteCreateTicket, type DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { strictRecord, requiredString, assertExactKeys } from "../lib/strictJsonContract";
import { parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import type { CreateConnectorInviteRequest } from "../types/generated/CreateConnectorInviteRequest";
import type { CreatedConnectorInvite } from "../types/generated/CreatedConnectorInvite";

export type ConnectorInviteCustody = {
  authority: DesktopManagerRoomAuthority;
  result: CreatedConnectorInvite;
  origin: string;
  expiresAtMs: number;
};

export async function createConnectorInvite(
  authority: DesktopManagerRoomAuthority,
  request: CreateConnectorInviteRequest,
  assertCurrent: () => void,
): Promise<ConnectorInviteCustody> {
  const ticket = await requestDesktopConnectorInviteCreateTicket(authority);
  assertCurrent();
  const response = await fetch(`${ticket.http_base_url}/api/room-connector/invite`, {
    method: "POST", cache: "no-store", redirect: "error",
    headers: { Authorization: `Bearer ${ticket.ticket}`, "Content-Type": "application/json" },
    body: JSON.stringify(request),
  });
  if (!response.ok) throw new Error("외부 AI 초대 생성 결과를 확인하지 못했어요. 다시 시도해 주세요.");
  const value = strictRecord(await response.json(), "외부 AI 초대");
  assertExactKeys(value, ["request_id", "room_uid", "invite_id", "expires_at", "join_url"], "외부 AI 초대");
  const result: CreatedConnectorInvite = {
    request_id: requiredString(value, "request_id", "외부 AI 초대"),
    room_uid: requiredString(value, "room_uid", "외부 AI 초대"),
    invite_id: requiredString(value, "invite_id", "외부 AI 초대"),
    expires_at: requiredString(value, "expires_at", "외부 AI 초대"),
    join_url: requiredString(value, "join_url", "외부 AI 초대"),
  };
  const url = new URL(result.join_url);
  parsePublicIngressOrigin(url.origin);
  const expiresAtMs = Date.parse(result.expires_at);
  if (result.request_id !== request.request_id || result.room_uid !== authority.room_uid ||
      !Number.isFinite(expiresAtMs) || url.pathname !== "/join" || url.username || url.password || url.hash ||
      url.searchParams.getAll("token").length !== 1) {
    throw new Error("외부 AI 초대 응답을 확인할 수 없어요. 다시 시도해 주세요.");
  }
  return { authority, result, origin: url.origin, expiresAtMs };
}
