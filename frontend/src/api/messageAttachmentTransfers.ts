import { fetchRoomSettings } from "./room";
import { uploadMessageAttachment, parseMessageAttachment, type LobbyAttachmentRef, type MessageAttachmentAuthority } from "./messageAttachments";
import { requestDesktopMessageAttachmentUploadTicket } from "../lib/desktopBridge";
import { fetchProductTransport, messageAttachmentCapabilities } from "../lib/remote/remoteWorkspace";
import { messageAttachmentId } from "../lib/messageAttachmentId";
import { canonicalRoomId } from "../lib/canonicalRoomId";
import { strictRecord, assertExactKeys } from "../lib/strictJsonContract";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";
import { isPrivateNoStoreResponse } from "./http";

export type TransferStatus = Readonly<{
  handle: string; attachment_id: string; piece_size: number; expires_at: number;
  declared_size: number; accepted_offset: number; attachment: LobbyAttachmentRef | null;
}>;
export class UploadFailure extends Error {
  constructor(readonly code: string, readonly retryable: boolean, readonly retryAfterMs?: number) {
    super(code);
  }
}
export interface MessageAttachmentUploadClient {
  readonly limit: number;
  readonly transfer: boolean;
  /** The room's per-file limit as last seen; refreshed at every begin. */
  readonly fileLimit: number;
  preflight(file: File): void;
  begin(file: File, operationId: string, signal: AbortSignal): Promise<TransferStatus>;
  data(handle: string, offset: number, body: Blob, signal: AbortSignal): Promise<TransferStatus>;
  finish(handle: string, signal: AbortSignal): Promise<TransferStatus>;
  status(handle: string, signal: AbortSignal): Promise<TransferStatus>;
  cancel(handle: string): Promise<void>;
  legacy(file: File, signal: AbortSignal): Promise<LobbyAttachmentRef>;
}

function parseStatus(value: unknown): TransferStatus {
  const envelope = strictRecord(value, "upload response");
  assertExactKeys(envelope, ["transfer"], "upload response");
  const status = strictRecord(envelope.transfer, "upload status");
  assertExactKeys(status, ["handle", "attachment_id", "piece_size", "expires_at", "declared_size", "accepted_offset", "attachment"], "upload status");
  for (const key of ["piece_size", "expires_at", "declared_size", "accepted_offset"] as const) {
    if (!Number.isSafeInteger(status[key]) || Number(status[key]) < (key === "accepted_offset" ? 0 : 1)) throw new UploadFailure("invalid_upload_response", false);
  }
  if (typeof status.handle !== "string" || !status.handle || status.handle.length > 128 ||
      typeof status.attachment_id !== "string" ||
      Number(status.piece_size) > 64 * 1024 || Number(status.accepted_offset) > Number(status.declared_size)) throw new UploadFailure("invalid_upload_response", false);
  messageAttachmentId(status.attachment_id as string);
  const attachment = status.attachment === null ? null : parseMessageAttachment(status.attachment);
  if (attachment && (attachment.id !== status.attachment_id || attachment.size !== status.declared_size || status.accepted_offset !== status.declared_size)) throw new UploadFailure("invalid_upload_response", false);
  return { ...status, attachment } as TransferStatus;
}

// Prepare at room admission (never at file selection). Capability comes from this
// host's response, not the installed frontend's version.
export async function createMessageAttachmentUploadClient(
  roomId: string, authority: MessageAttachmentAuthority, signal?: AbortSignal
): Promise<MessageAttachmentUploadClient> {
  signal?.throwIfAborted();
  const room = canonicalRoomId(roomId);
  const settingsAuthority = authority.kind === "remote" ? authority : {};
  const settings = await fetchRoomSettings(room, settingsAuthority);
  signal?.throwIfAborted();
  async function grant() {
    if (authority.kind === "local") {
      const ticket = await requestDesktopMessageAttachmentUploadTicket(room);
      return { base: ticket.http_base_url, token: ticket.ticket, device: undefined };
    }
    return { base: "", token: authority.sessionToken, device: authority.deviceToken };
  }
  const discovery = await grant();
  if (authority.kind === "local") {
    // POST-only on both old and new hosts: GET discovers response capabilities
    // without consuming the purpose-bound one-use upload ticket or creating work.
    const response = await fetchProductTransport(`${discovery.base}/api/message-attachments`, { cache: "no-store", signal });
    await response.body?.cancel();
  }
  const capabilities = messageAttachmentCapabilities(discovery.token, `${discovery.base}/api/message-attachments`);
  let fileLimit = settings.attachmentFileLimitBytes ?? MAX_ATTACHMENT_BYTES;
  function preflight(file: File) {
    if (!Number.isSafeInteger(file.size) || file.size < 1 || file.size > fileLimit) throw new UploadFailure("file_size_limit", false);
  }
  let initialGrant: Awaited<ReturnType<typeof grant>> | undefined = discovery;
  async function request(path: string, method: string, signal?: AbortSignal, body?: Blob | string): Promise<unknown> {
    if (!capabilities.transfer) throw new UploadFailure("upload_unsupported", false);
    signal?.throwIfAborted();
    const pending = initialGrant;
    initialGrant = undefined;
    const resolved = pending ?? await grant();
    signal?.throwIfAborted();
    const headers = new Headers({ Authorization: `Bearer ${resolved.token}` });
    if (resolved.device) headers.set("X-Device-Token", resolved.device);
    if (body !== undefined) headers.set("Content-Type", typeof body === "string" ? "application/json" : "application/octet-stream");
    const response = await fetchProductTransport(`${resolved.base}/api/message-attachment-transfers${path}`, { method, headers, body, cache: "no-store", signal });
    if (!isPrivateNoStoreResponse(response, "application/json")) throw new UploadFailure("invalid_upload_response", false);
    const value = await response.json();
    if (!response.ok) {
      const error = strictRecord(value, "upload error");
      if (typeof error.code !== "string") throw new UploadFailure("invalid_upload_response", false);
      if (error.code === "upload_deferred" && (error.disposition !== "defer" || !Number.isSafeInteger(error.retry_after_ms) || Number(error.retry_after_ms) < 1)) throw new UploadFailure("invalid_upload_response", false);
      throw new UploadFailure(error.code, response.status >= 500 || response.status === 429 || ["upload_request_busy", "upload_offset_conflict", "upload_interrupted", "upload_missing"].includes(error.code), error.code === "upload_deferred" ? Number(error.retry_after_ms) : undefined);
    }
    return value;
  }
  const target = (handle: string) => `/${encodeURIComponent(handle)}`;
  return {
    limit: capabilities.limit, transfer: capabilities.transfer, preflight,
    get fileLimit() { return fileLimit; },
    begin: async (file, operationId, signal) => {
      signal.throwIfAborted();
      const current = await fetchRoomSettings(room, settingsAuthority);
      signal.throwIfAborted();
      fileLimit = current.attachmentFileLimitBytes ?? MAX_ATTACHMENT_BYTES;
      preflight(file);
      return parseStatus(await request("", "POST", signal, JSON.stringify({ room_id: room, operation_id: operationId, filename: file.name || "attachment.bin", content_type: file.type || "application/octet-stream", declared_size: file.size })));
    },
    data: async (handle, offset, body, signal) => parseStatus(await request(`${target(handle)}/data?offset=${offset}`, "POST", signal, body)),
    finish: async (handle, signal) => parseStatus(await request(`${target(handle)}/finish`, "POST", signal)),
    status: async (handle, signal) => parseStatus(await request(target(handle), "GET", signal)),
    cancel: async handle => {
      const result = strictRecord(await request(target(handle), "DELETE"), "upload cancel");
      assertExactKeys(result, ["cancelled"], "upload cancel");
      if (result.cancelled !== true) throw new UploadFailure("invalid_upload_response", false);
    },
    legacy: (file, signal) => uploadMessageAttachment(file, room, authority, undefined, signal),
  };
}
