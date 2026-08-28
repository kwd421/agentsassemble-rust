import { fileToBase64, responseError } from "./http";
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
import { MAX_RASTER_BYTES } from "../types/generated/ROOM_APPEARANCE_WIRE";

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
const PNG_SIGNATURE = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);

function invalidResponse(): never {
  throw new Error("방 외형 자산 응답 계약이 올바르지 않습니다.");
}

function requireResponseMetadata(response: Response, contentType: string) {
  const cacheDirectives = response.headers
    .get("Cache-Control")
    ?.split(",")
    .map((directive) => directive.trim().toLowerCase());
  if (
    response.headers.get("Content-Type") !== contentType ||
    cacheDirectives?.length !== 2 ||
    new Set(cacheDirectives).size !== 2 ||
    !cacheDirectives.includes("private") ||
    !cacheDirectives.includes("no-store")
  ) {
    invalidResponse();
  }
}

async function strictPngBlob(response: Response): Promise<Blob> {
  requireResponseMetadata(response, "image/png");
  const blob = await response.blob();
  if (blob.size < PNG_SIGNATURE.length || blob.size > MAX_RASTER_BYTES) {
    invalidResponse();
  }
  const signature = new Uint8Array(
    await blob.slice(0, PNG_SIGNATURE.length).arrayBuffer()
  );
  if (signature.some((byte, index) => byte !== PNG_SIGNATURE[index])) {
    invalidResponse();
  }
  return blob;
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

function exactSessionTicket(value: unknown): string {
  const grant = exactObject(value, ["ticket", "ttl_seconds"]);
  if (
    typeof grant.ticket !== "string" ||
    !/^[0-9a-f]{64}$/.test(grant.ticket) ||
    !Number.isSafeInteger(grant.ttl_seconds) ||
    Number(grant.ttl_seconds) < 1
  ) {
    invalidResponse();
  }
  return grant.ticket;
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
  const exchange = await fetch(
    `/api/session-tickets/room-appearance/${reference.assetId}`,
    {
      cache: "no-store",
      method: "POST",
      headers: bearer(authority.sessionToken),
      signal,
    }
  );
  if (!exchange.ok) throw await responseError(exchange);
  requireResponseMetadata(exchange, "application/json");
  const ticket = exactSessionTicket(await exchange.json());
  return fetch(reference.url, {
    cache: "no-store",
    headers: bearer(ticket),
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
