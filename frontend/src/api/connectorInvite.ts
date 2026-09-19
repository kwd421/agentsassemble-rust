import { requestDesktopConnectorInviteCreateTicket, type DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { strictRecord, requiredString, assertExactKeys } from "../lib/strictJsonContract";
import { parseLocalIngressOrigin, parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import type { CreateConnectorInviteRequest } from "../types/generated/CreateConnectorInviteRequest";
import type { CreatedConnectorInvite } from "../types/generated/CreatedConnectorInvite";
import type { InviteReach } from "../types/generated/InviteReach";

export type ConnectorInviteCustody = {
  authority: DesktopManagerRoomAuthority;
  result: CreatedConnectorInvite;
  origin: string;
  reach: InviteReach;
  expiresAtMs: number;
};

/** Clipboard / join-page text for the current-conversation MCP path. Not a webpage. */
export function connectorInviteText(joinUrl: string, expiresAt?: string): string {
  const expiry = expiresAt
    ? `한 번 사용할 수 있어요. 만료: ${new Date(expiresAt).toLocaleString()}`
    : "한 번 사용할 수 있고 1시간 뒤 만료돼요.";
  return [
    "현재 AI 대화용 AgentsAssemble Room Connector 초대입니다.",
    "이 URL을 열거나 fetch 하지 마세요. HTTP로 /api/room-connector/join 하지 마세요.",
    "",
    "이 작업 폴더에 Room Connector MCP를 등록하세요:",
    "assemble room connector-mcp",
    "Grok: grok mcp add --scope project agentsassemble <assemble 실행 파일> -- room connector-mcp",
    "",
    "폴더를 신뢰한 뒤 이 대화를 다시 열고, 아래 URL을 그대로 쓰세요.",
    joinUrl,
    "",
    expiry,
  ].join("\n");
}

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
  // A local link must stay on this machine's loopback listener; a public link must never.
  if (request.reach === "local") parseLocalIngressOrigin(url.origin);
  else parsePublicIngressOrigin(url.origin);
  const expiresAtMs = Date.parse(result.expires_at);
  if (result.request_id !== request.request_id || result.room_uid !== authority.room_uid ||
      !Number.isFinite(expiresAtMs) || url.pathname !== "/join" || url.username || url.password || url.hash ||
      url.searchParams.getAll("token").length !== 1) {
    throw new Error("외부 AI 초대 응답을 확인할 수 없어요. 다시 시도해 주세요.");
  }
  return { authority, result, origin: url.origin, reach: request.reach, expiresAtMs };
}
