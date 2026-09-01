import {
  fileToBase64,
  isPrivateNoStoreResponse,
  responseError,
} from "./http";
import {
  requestDesktopAppearanceBoundReadTicket,
  requestDesktopAppearancePendingReadTicket,
  requestDesktopAppearanceUploadTicket,
  type DesktopManagerRoomAuthority,
  type DesktopOperatorHttpTicket,
} from "../lib/desktopBridge";
import {
  roomAppearanceAssetReference,
  type RoomAppearanceAssetReference,
} from "../lib/roomAppearanceAsset";
import { strictPrivatePngBlob } from "./safeRaster";

export type RoomAppearanceReadAuthority =
  | { kind: "local"; manager: DesktopManagerRoomAuthority }
  | { kind: "remote"; sessionToken: string };

export type UploadedRoomAppearance = Readonly<{
  reference: RoomAppearanceAssetReference;
  filename: string;
  size: number;
}>;

const ATTACHMENT_KEYS = [
  "id",
  "filename",
  "content_type",
  "size",
  "is_image",
  "url",
] as const;
function invalidResponse(): never {
  throw new Error("방 외형 자산 응답 계약이 올바르지 않습니다.");
}

function requireResponseMetadata(response: Response, contentType: string) {
  if (!isPrivateNoStoreResponse(response, contentType)) invalidResponse();
}

async function strictPngBlob(response: Response): Promise<Blob> {
  return strictPrivatePngBlob(response, "방 외형 자산 응답 계약이 올바르지 않습니다.");
}

function exactObject(
  value: unknown,
  keys: readonly string[]
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalidResponse();
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    invalidResponse();
  }
  return record;
}

function parseUploadResponse(value: unknown): UploadedRoomAppearance {
  const envelope = exactObject(value, ["attachment"]);
  const attachment = exactObject(envelope.attachment, ATTACHMENT_KEYS);
  if (
    typeof attachment.id !== "string" ||
    typeof attachment.url !== "string" ||
    typeof attachment.filename !== "string" ||
    !attachment.filename ||
    attachment.content_type !== "image/png" ||
    attachment.is_image !== true ||
    !Number.isSafeInteger(attachment.size) ||
    Number(attachment.size) < 1
  ) {
    invalidResponse();
  }
  let reference: RoomAppearanceAssetReference;
  try {
    reference = roomAppearanceAssetReference(attachment.url);
  } catch {
    invalidResponse();
  }
  if (reference.assetId !== attachment.id) invalidResponse();
  return Object.freeze({
    reference,
    filename: attachment.filename,
    size: attachment.size as number,
  });
}

function bearer(ticket: string): Headers {
  const headers = new Headers();
  headers.set("Authorization", `Bearer ${ticket}`);
  return headers;
}

async function fetchLocalAppearance(
  reference: RoomAppearanceAssetReference,
  authority: Extract<RoomAppearanceReadAuthority, { kind: "local" }>,
  mode: "pending" | "bound",
  signal?: AbortSignal
): Promise<Response> {
  const grant: DesktopOperatorHttpTicket =
    mode === "pending"
      ? await requestDesktopAppearancePendingReadTicket(
          authority.manager,
          reference.assetId
        )
      : await requestDesktopAppearanceBoundReadTicket(
          authority.manager,
          reference.assetId
        );
  return fetch(`${grant.http_base_url}${reference.url}`, {
    cache: "no-store",
    headers: bearer(grant.ticket),
    signal,
  });
}

async function fetchRemoteAppearance(
  reference: RoomAppearanceAssetReference,
  authority: Extract<RoomAppearanceReadAuthority, { kind: "remote" }>,
  signal?: AbortSignal
): Promise<Response> {
  if (!authority.sessionToken) {
    throw new Error("방 세션 권위를 사용할 수 없습니다.");
  }
  return fetch(reference.url, {
    cache: "no-store",
    headers: bearer(authority.sessionToken),
    signal,
  });
}

export async function uploadRoomAppearance(
  file: File,
  manager: DesktopManagerRoomAuthority
): Promise<UploadedRoomAppearance> {
  const dataBase64 = await fileToBase64(file);
  const grant = await requestDesktopAppearanceUploadTicket(manager);
  const response = await fetch(`${grant.http_base_url}/api/attachments`, {
    cache: "no-store",
    method: "POST",
    headers: new Headers({
      Authorization: `Bearer ${grant.ticket}`,
      "Content-Type": "application/json",
    }),
    body: JSON.stringify({
      purpose: "room_appearance",
      filename: file.name || "room-appearance.png",
      content_type: file.type || "application/octet-stream",
      data_base64: dataBase64,
    }),
  });
  if (!response.ok) throw await responseError(response);
  requireResponseMetadata(response, "application/json");
  return parseUploadResponse(await response.json());
}

export async function fetchRoomAppearanceBlob(
  canonicalReference: string,
  authority: RoomAppearanceReadAuthority,
  mode: "pending" | "bound",
  signal?: AbortSignal
): Promise<Blob> {
  const reference = roomAppearanceAssetReference(canonicalReference);
  if (authority.kind === "remote" && mode === "pending") {
    throw new Error("원격 방 세션은 pending 외형 자산을 읽을 수 없습니다.");
  }
  const response =
    authority.kind === "local"
      ? await fetchLocalAppearance(reference, authority, mode, signal)
      : await fetchRemoteAppearance(reference, authority, signal);
  if (!response.ok) throw await responseError(response);
  return strictPngBlob(response);
}
