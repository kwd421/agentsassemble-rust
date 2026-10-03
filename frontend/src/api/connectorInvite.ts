import { fetchRemoteRoomInvite, type RemoteInviteTransport } from "./roomInviteTransport";
import { requestDesktopConnectorInviteCreateTicket, type DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { strictRecord, requiredString, assertExactKeys } from "../lib/strictJsonContract";
import { parseLocalIngressOrigin, parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import type { CreateConnectorInviteRequest } from "../types/generated/CreateConnectorInviteRequest";
import type { CreatedConnectorInvite } from "../types/generated/CreatedConnectorInvite";
import type { InviteReach } from "../types/generated/InviteReach";
import instructions from "./connectorInviteInstructions.txt?raw";

export type ConnectorInviteCustody = {
  authority: DesktopManagerRoomAuthority;
  result: CreatedConnectorInvite;
  origin: string;
  reach: InviteReach;
  expiresAtMs: number;
};

/** One instruction source for the clipboard and server-rendered invitation document. */
export function connectorInviteText(joinUrl: string, expiresAt?: string): string {
  const origin = new URL(joinUrl).origin;
  if (!/^[A-Za-z0-9:/._\[\]-]+$/.test(origin)) {
    throw new Error("MCP 등록 안내에 사용할 서버 주소가 올바르지 않아요.");
  }
  const expiry = expiresAt
    ? `만료: ${new Date(expiresAt).toLocaleString()}`
    : "만료 여부는 room_join에서 확인합니다.";
  return instructions.replaceAll("{{MCP_URL}}", `${origin}/mcp`)
    .replaceAll("{{INVITE_URL}}", joinUrl).replaceAll("{{EXPIRY}}", expiry);
}

export async function createConnectorInvite(
  authority: DesktopManagerRoomAuthority,
  request: CreateConnectorInviteRequest,
  assertCurrent: () => void,
  remote?: RemoteInviteTransport,
): Promise<ConnectorInviteCustody> {
  const response = remote
    ? await fetchRemoteRoomInvite("/room-connector/invite", remote, {
        method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(request),
      }, assertCurrent)
    : await (async () => {
        const ticket = await requestDesktopConnectorInviteCreateTicket(authority);
        assertCurrent();
        return fetch(`${ticket.http_base_url}/api/room-connector/invite`, {
          method: "POST", cache: "no-store", redirect: "error",
          headers: { Authorization: `Bearer ${ticket.ticket}`, "Content-Type": "application/json" },
          body: JSON.stringify(request),
        });
      })();
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
