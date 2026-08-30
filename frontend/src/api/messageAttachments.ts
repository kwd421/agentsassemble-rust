import {
  requestDesktopMessageAttachmentReadTicket,
  requestDesktopMessageAttachmentUploadTicket,
} from "../lib/desktopBridge";
import { canonicalRoomId } from "../lib/canonicalRoomId";
import {
  messageAttachmentId,
  messageAttachmentReference,
} from "../lib/messageAttachmentId";
import {
  assertExactKeys,
  strictRecord,
} from "../lib/strictJsonContract";
import { isUnicodeScalarString } from "../lib/unicodeScalarString";
import { trimRustWhitespace } from "../lib/rustWhitespace";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";
import {
  MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES,
  MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS,
} from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";
import {
  fileToBase64,
  isPrivateNoStoreResponse,
  parseSessionHttpTicket,
  responseError,
} from "./http";
import type { RoomHttpAuthority } from "./roomHttpAuthority";

export type LobbyAttachmentRef = Readonly<{
  id: string;
  filename: string;
  content_type: string;
  size: number;
  is_image: boolean;
  url: string;
  download_url: string;
}>;

export type MessageAttachmentAuthority = RoomHttpAuthority;

type TransferGrant = Readonly<{ baseUrl: string; ticket: string }>;

const ATTACHMENT_KEYS = [
  "id",
  "filename",
  "content_type",
  "size",
  "is_image",
  "url",
  "download_url",
] as const;

function invalidResponse(): never {
  throw new Error("로비 메시지 첨부 응답 계약이 올바르지 않습니다.");
}

export function parseMessageAttachmentFilename(value: unknown): string {
  if (typeof value !== "string" || !isUnicodeScalarString(value)) invalidResponse();
  const characters = [...value];
  if (
    !value ||
    value === "." ||
    value === ".." ||
    characters.length > MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS ||
    characters.some(
      (character) => /\p{Cc}/u.test(character) || character === "/" || character === "\\"
    ) ||
    trimRustWhitespace(value) !== value
  ) {
    invalidResponse();
  }
  return value;
}

function canonicalContentType(value: unknown): string {
  if (
    typeof value !== "string" ||
    new TextEncoder().encode(value).byteLength >
      MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES ||
    !/^[a-z0-9.+-]+\/[a-z0-9.+-]+$/.test(value)
  ) {
    invalidResponse();
  }
  return value;
}

export function parseMessageAttachment(value: unknown): LobbyAttachmentRef {
  const attachment = strictRecord(value, "로비 메시지 첨부");
  assertExactKeys(attachment, ATTACHMENT_KEYS, "로비 메시지 첨부");
  let id: string;
  if (typeof attachment.id !== "string") invalidResponse();
  try {
    id = messageAttachmentId(attachment.id);
  } catch {
    invalidResponse();
  }
  const filename = parseMessageAttachmentFilename(attachment.filename);
  const contentType = canonicalContentType(attachment.content_type);
  const size = attachment.size;
  if (
    !Number.isSafeInteger(size) ||
    Number(size) < 1 ||
    Number(size) > MAX_ATTACHMENT_BYTES ||
    typeof attachment.is_image !== "boolean" ||
    attachment.url !== messageAttachmentReference(id, "view") ||
    attachment.download_url !== messageAttachmentReference(id, "download")
  ) {
    invalidResponse();
  }
  return Object.freeze({
    id,
    filename,
    content_type: contentType,
    size: size as number,
    is_image: attachment.is_image,
    url: attachment.url as string,
    download_url: attachment.download_url as string,
  });
}

function parseUploadResponse(value: unknown): LobbyAttachmentRef {
  const envelope = strictRecord(value, "로비 메시지 첨부 업로드");
  assertExactKeys(envelope, ["attachment"], "로비 메시지 첨부 업로드");
  return parseMessageAttachment(envelope.attachment);
}

function bearer(ticket: string, json = false): Headers {
  const headers = new Headers({ Authorization: `Bearer ${ticket}` });
  if (json) headers.set("Content-Type", "application/json");
  return headers;
}

async function remoteGrant(
  path: string,
  sessionToken: string,
  signal?: AbortSignal
): Promise<TransferGrant> {
  if (!sessionToken) throw new Error("방 세션 권위를 사용할 수 없습니다.");
  const exchange = await fetch(path, {
    cache: "no-store",
    method: "POST",
    headers: bearer(sessionToken),
    signal,
  });
  if (!exchange.ok) throw await responseError(exchange);
  if (!isPrivateNoStoreResponse(exchange, "application/json")) invalidResponse();
  let ticket: string;
  try {
    ticket = parseSessionHttpTicket(await exchange.json());
  } catch {
    invalidResponse();
  }
  return { baseUrl: "", ticket };
}

async function uploadGrant(
  roomId: string,
  authority: MessageAttachmentAuthority,
  signal?: AbortSignal
): Promise<TransferGrant> {
  signal?.throwIfAborted();
  if (authority.kind === "local") {
    const grant = await requestDesktopMessageAttachmentUploadTicket(roomId);
    signal?.throwIfAborted();
    return { baseUrl: grant.http_base_url, ticket: grant.ticket };
  }
  const grant = await remoteGrant(
    "/api/session-tickets/message-attachment-upload",
    authority.sessionToken,
    signal
  );
  signal?.throwIfAborted();
  return grant;
}

async function readGrant(
  roomId: string,
  attachmentId: string,
  authority: MessageAttachmentAuthority,
  signal?: AbortSignal
): Promise<TransferGrant> {
  signal?.throwIfAborted();
  if (authority.kind === "local") {
    const grant = await requestDesktopMessageAttachmentReadTicket(
      roomId,
      attachmentId
    );
    signal?.throwIfAborted();
    return { baseUrl: grant.http_base_url, ticket: grant.ticket };
  }
  const grant = await remoteGrant(
    `/api/session-tickets/message-attachment/${attachmentId}`,
    authority.sessionToken,
    signal
  );
  signal?.throwIfAborted();
  return grant;
}

export async function uploadMessageAttachment(
  file: File,
  roomId: string,
  authority: MessageAttachmentAuthority,
  beforeDispatch?: () => void,
  signal?: AbortSignal
): Promise<LobbyAttachmentRef> {
  const canonicalRoom = canonicalRoomId(roomId);
  if (!Number.isSafeInteger(file.size) || file.size < 1 || file.size > MAX_ATTACHMENT_BYTES) {
    throw new Error("메시지 첨부는 1바이트 이상 10MiB 이하여야 합니다.");
  }
  const grant = await uploadGrant(canonicalRoom, authority, signal);
  const dataBase64 = await fileToBase64(file, signal);
  signal?.throwIfAborted();
  beforeDispatch?.();
  const response = await fetch(`${grant.baseUrl}/api/attachments`, {
    cache: "no-store",
    method: "POST",
    headers: bearer(grant.ticket, true),
    body: JSON.stringify({
      purpose: "room_attachment",
      filename: file.name || "attachment.bin",
      content_type: file.type || "application/octet-stream",
      data_base64: dataBase64,
    }),
    signal,
  });
  if (!response.ok) throw await responseError(response);
  if (!isPrivateNoStoreResponse(response, "application/json")) invalidResponse();
  return parseUploadResponse(await response.json());
}

export async function fetchMessageAttachmentBlob(
  attachmentValue: LobbyAttachmentRef,
  roomId: string,
  authority: MessageAttachmentAuthority,
  mode: "view" | "download",
  signal?: AbortSignal,
  beforeDispatch?: () => void
): Promise<Blob> {
  const canonicalRoom = canonicalRoomId(roomId);
  const attachment = parseMessageAttachment(attachmentValue);
  const grant = await readGrant(canonicalRoom, attachment.id, authority, signal);
  const reference =
    mode === "view" ? attachment.url : attachment.download_url;
  beforeDispatch?.();
  const response = await fetch(`${grant.baseUrl}${reference}`, {
    cache: "no-store",
    headers: bearer(grant.ticket),
    signal,
  });
  if (!response.ok) throw await responseError(response);
  if (!isPrivateNoStoreResponse(response, attachment.content_type)) invalidResponse();
  const blob = await response.blob();
  if (
    blob.size !== attachment.size ||
    blob.size < 1 ||
    blob.size > MAX_ATTACHMENT_BYTES ||
    blob.type !== attachment.content_type
  ) {
    invalidResponse();
  }
  return blob;
}
