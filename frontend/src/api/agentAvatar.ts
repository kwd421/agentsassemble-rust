import { fileToBase64, isPrivateNoStoreResponse, responseError } from "./http";
import { requestDesktopAgentAvatarUploadTicket, type DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { parseAgentAvatarReference } from "../lib/agentAvatarReference";
import { assertExactKeys, strictRecord } from "../lib/strictJsonContract";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";

export async function uploadAgentAvatar(file: File, manager: DesktopManagerRoomAuthority,
  sessionId: string, signal: AbortSignal): Promise<string> {
  if (file.size < 1 || file.size > MAX_ATTACHMENT_BYTES) throw new Error("프로필 사진 크기가 허용 범위를 벗어났습니다.");
  const data = await fileToBase64(file, signal);
  const grant = await requestDesktopAgentAvatarUploadTicket(manager, sessionId);
  signal.throwIfAborted();
  const response = await fetch(`${grant.http_base_url}/api/agent-avatars/upload/${encodeURIComponent(sessionId)}`, {
    method: "POST", cache: "no-store", redirect: "error", signal,
    headers: { Authorization: `Bearer ${grant.ticket}`, "Content-Type": "application/json" },
    body: JSON.stringify({ filename: file.name, content_type: file.type, data_base64: data }),
  });
  if (!response.ok) throw await responseError(response);
  const label = "에이전트 프로필 사진";
  if (!isPrivateNoStoreResponse(response, "application/json")) throw new Error(`${label} 응답이 올바르지 않습니다.`);
  const envelope = strictRecord(await response.json(), label);
  assertExactKeys(envelope, ["attachment"], label);
  const attachment = strictRecord(envelope.attachment, label);
  assertExactKeys(attachment, ["id", "filename", "content_type", "size", "is_image", "url"], label);
  const reference = parseAgentAvatarReference(attachment.url);
  if (!reference || reference.assetId !== attachment.id || attachment.content_type !== "image/png" ||
    attachment.is_image !== true || typeof attachment.filename !== "string" || !attachment.filename ||
    !Number.isSafeInteger(attachment.size) || Number(attachment.size) < 1 || Number(attachment.size) > MAX_ATTACHMENT_BYTES) {
    throw new Error(`${label} 응답이 올바르지 않습니다.`);
  }
  return reference.url;
}
