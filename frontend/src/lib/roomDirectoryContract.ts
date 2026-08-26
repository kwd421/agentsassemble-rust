import type { ServerRoomDockSource } from "./roomDockModel";
import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { lengthDelimitedTranscript, sha256Hex } from "./lengthDelimitedCrypto";
import {
  assertExactKeys as exactKeys,
  requiredString,
  strictRecord as record,
} from "./strictJsonContract";

export type StrictRoomDirectory = {
  server_id: string;
  authority_lineage_id: string;
  server_product_surface: ServerProductSurface;
  rooms: ServerRoomDockSource[];
};

export type RoomDirectoryAuthority = Pick<
  StrictRoomDirectory,
  "server_id" | "authority_lineage_id"
>;

export type TrustedServerProductSurface = Pick<
  ServerProductSurface,
  "revision" | "digest"
>;

export type RoomSessionSurface = RoomDirectoryAuthority & {
  server_product_surface: ServerProductSurface;
};

export type StrictRoomCreateResponse = RoomDirectoryAuthority & {
  status: "ready";
  room: ServerRoomDockSource;
  deduplicated: boolean;
};

let boundAuthority: { origin: string; authority: RoomDirectoryAuthority } | null = null;
let boundSurface: { origin: string; surface: ServerProductSurface } | null = null;

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function validateAppearance(value: unknown, label: string) {
  const appearance = record(value, label);
  exactKeys(
    appearance,
    [
      "banner_preset",
      "banner_image_url",
      "icon_image_url",
      "icon_label",
      "invite_scope",
    ],
    label
  );
  for (const key of Object.keys(appearance)) {
    if (typeof appearance[key] !== "string") {
      throw new Error(`${label}.${key}가 올바르지 않습니다.`);
    }
  }
}

function validateChannels(value: unknown, label: string) {
  if (!Array.isArray(value)) throw new Error(`${label}가 배열이 아닙니다.`);
  value.forEach((entry, index) => {
    const channel = record(entry, `${label}[${index}]`);
    exactKeys(channel, ["id", "name", "type", "position", "created_at"], label);
    requiredString(channel, "id", label);
    requiredString(channel, "name", label);
    requiredString(channel, "type", label);
    requiredString(channel, "created_at", label);
    if (!Number.isSafeInteger(channel.position) || Number(channel.position) < 0) {
      throw new Error(`${label}.position이 올바르지 않습니다.`);
    }
  });
}

function validateSettings(value: unknown, roomId: string, label: string) {
  const settings = record(value, label);
  exactKeys(
    settings,
    [
      "room_id",
      "settings_revision",
      "label",
      "topic",
      "appearance",
      "conversation_mode",
      "tool_mode",
      "ordered_exclude_previous_speaker",
      "channels",
      "activity_plugin",
    ],
    label
  );
  if (requiredString(settings, "room_id", label) !== roomId) {
    throw new Error(`${label}.room_id가 방 권위와 일치하지 않습니다.`);
  }
  for (const key of [
    "settings_revision",
    "label",
    "topic",
    "conversation_mode",
    "tool_mode",
    "activity_plugin",
  ]) {
    if (typeof settings[key] !== "string") {
      throw new Error(`${label}.${key}가 올바르지 않습니다.`);
    }
  }
  if (typeof settings.ordered_exclude_previous_speaker !== "boolean") {
    throw new Error(`${label}.ordered_exclude_previous_speaker가 올바르지 않습니다.`);
  }
  validateAppearance(settings.appearance, `${label}.appearance`);
  validateChannels(settings.channels, `${label}.channels`);
}

function validateRoom(value: unknown, index: number): ServerRoomDockSource {
  const label = `rooms[${index}]`;
  const room = record(value, label);
  exactKeys(
    room,
    [
      "room_id",
      "room_uid",
      "label",
      "last_active_at",
      "archived",
      "status",
      "origin",
      "room_settings",
    ],
    label
  );
  const roomId = requiredString(room, "room_id", label);
  if (!UUID_PATTERN.test(requiredString(room, "room_uid", label))) {
    throw new Error(`${label}.room_uid가 UUID가 아닙니다.`);
  }
  requiredString(room, "label", label);
  requiredString(room, "last_active_at", label);
  requiredString(room, "origin", label);
  if (typeof room.archived !== "boolean") {
    throw new Error(`${label}.archived가 올바르지 않습니다.`);
  }
  if (!new Set(["active", "closed", "archived"]).has(requiredString(room, "status", label))) {
    throw new Error(`${label}.status가 올바르지 않습니다.`);
  }
  validateSettings(room.room_settings, roomId, `${label}.room_settings`);
  return room as ServerRoomDockSource;
}

function validateCreatedRoom(value: unknown): ServerRoomDockSource {
  const room = record(value, "생성된 방");
  exactKeys(
    room,
    [
      "room_id",
      "room_uid",
      "label",
      "last_active_at",
      "archived",
      "status",
      "origin",
    ],
    "생성된 방"
  );
  requiredString(room, "room_id", "생성된 방");
  if (!UUID_PATTERN.test(requiredString(room, "room_uid", "생성된 방"))) {
    throw new Error("생성된 방.room_uid가 UUID가 아닙니다.");
  }
  requiredString(room, "label", "생성된 방");
  requiredString(room, "last_active_at", "생성된 방");
  requiredString(room, "origin", "생성된 방");
  if (typeof room.archived !== "boolean") {
    throw new Error("생성된 방.archived가 올바르지 않습니다.");
  }
  if (!new Set(["active", "closed", "archived"]).has(requiredString(room, "status", "생성된 방"))) {
    throw new Error("생성된 방.status가 올바르지 않습니다.");
  }
  return room as ServerRoomDockSource;
}

function validateAuthority(payload: Record<string, unknown>, label: string) {
  const serverId = requiredString(payload, "server_id", label);
  const lineageId = requiredString(payload, "authority_lineage_id", label);
  if (!UUID_PATTERN.test(serverId) || !UUID_PATTERN.test(lineageId)) {
    throw new Error(`${label}의 서버 또는 권위 계보 식별자가 UUID가 아닙니다.`);
  }
  return { server_id: serverId, authority_lineage_id: lineageId };
}

function validateServerProductSurface(value: unknown): ServerProductSurface {
  const surface = record(value, "서버 제품 표면");
  exactKeys(
    surface,
    ["revision", "digest", "http_routes", "websocket_streams", "websocket_actions"],
    "서버 제품 표면"
  );
  if (
    surface.revision !== PRODUCT_SURFACE_REVISION ||
    !/^[0-9a-f]{64}$/.test(String(surface.digest))
  ) {
    throw new Error("서버 제품 표면 revision 또는 digest가 올바르지 않습니다.");
  }
  if (!Array.isArray(surface.http_routes)) {
    throw new Error("서버 제품 표면 HTTP route가 배열이 아닙니다.");
  }
  const routes = surface.http_routes.map((value, index) => {
    const route = record(value, `서버 제품 표면 HTTP route[${index}]`);
    exactKeys(route, ["method", "path"], `서버 제품 표면 HTTP route[${index}]`);
    if (
      !new Set(["GET", "POST"]).has(String(route.method)) ||
      typeof route.path !== "string" ||
      !route.path.startsWith("/")
    ) {
      throw new Error(`서버 제품 표면 HTTP route[${index}]가 올바르지 않습니다.`);
    }
    return `${route.method} ${route.path}`;
  });
  const streams = surface.websocket_streams;
  const actions = surface.websocket_actions;
  if (
    !Array.isArray(streams) ||
    streams.some((stream) => stream !== "room_events") ||
    !Array.isArray(actions) ||
    actions.some(
      (action) =>
        typeof action !== "string" ||
        !new Set([
          "message.send",
          "participant.leave",
          "participant.mute",
          "participant.role.update",
          "room.settings.update",
          "room.random.roll",
          "room.random.choose",
          "agent.create",
          "agent.configure",
          "agent.start",
          "agent.resume",
          "agent.stop",
        ]).has(action)
    )
  ) {
    throw new Error("서버 제품 표면 WebSocket 등록부가 올바르지 않습니다.");
  }
  for (const values of [routes, streams, actions]) {
    const canonical = [...values].sort();
    if (
      canonical.length !== new Set(canonical).size ||
      canonical.some((entry, index) => entry !== values[index])
    ) {
      throw new Error("서버 제품 표면 등록부가 정렬되지 않았거나 중복되었습니다.");
    }
  }
  return surface as unknown as ServerProductSurface;
}

async function canonicalServerSurfaceDigest(
  surface: ServerProductSurface
): Promise<string> {
  const fields = [
    String(surface.revision),
    ...surface.http_routes.map((route) => `${route.method} ${route.path}`),
    "streams",
    ...surface.websocket_streams,
    "actions",
    ...surface.websocket_actions,
  ];
  return sha256Hex(
    lengthDelimitedTranscript(
      "agentsassemble.server-product-surface.v1",
      fields
    )
  );
}

async function assertServerProductSurfaceIntegrity(
  surface: ServerProductSurface,
  trusted: TrustedServerProductSurface | null
) {
  const computed = await canonicalServerSurfaceDigest(surface);
  if (computed !== surface.digest) {
    throw new Error("서버 제품 표면 digest가 등록부 내용과 일치하지 않습니다.");
  }
  if (
    trusted &&
    (trusted.revision !== surface.revision || trusted.digest !== surface.digest)
  ) {
    throw new Error("서버 제품 표면이 네이티브 bootstrap 권위와 일치하지 않습니다.");
  }
}

export function parseStrictRoomDirectory(value: unknown): StrictRoomDirectory {
  const payload = record(value, "방 목록");
  exactKeys(
    payload,
    ["server_id", "authority_lineage_id", "server_product_surface", "rooms"],
    "방 목록"
  );
  const authority = validateAuthority(payload, "방 목록");
  if (!Array.isArray(payload.rooms)) {
    throw new Error("방 목록 rooms가 배열이 아닙니다.");
  }
  return {
    ...authority,
    server_product_surface: validateServerProductSurface(payload.server_product_surface),
    rooms: payload.rooms.map(validateRoom),
  };
}

export function parseRoomSessionSurface(value: unknown): RoomSessionSurface {
  const payload = record(value, "방 세션 서버 표면");
  exactKeys(
    payload,
    ["server_id", "authority_lineage_id", "server_product_surface"],
    "방 세션 서버 표면"
  );
  return {
    ...validateAuthority(payload, "방 세션 서버 표면"),
    server_product_surface: validateServerProductSurface(payload.server_product_surface),
  };
}

export function parseStrictRoomCreateResponse(value: unknown): StrictRoomCreateResponse {
  const payload = record(value, "방 생성");
  exactKeys(
    payload,
    ["status", "server_id", "authority_lineage_id", "room", "deduplicated"],
    "방 생성"
  );
  if (payload.status !== "ready" || typeof payload.deduplicated !== "boolean") {
    throw new Error("방 생성 결과가 올바르지 않습니다.");
  }
  return {
    status: "ready",
    ...validateAuthority(payload, "방 생성"),
    room: validateCreatedRoom(payload.room),
    deduplicated: payload.deduplicated,
  };
}

export function assertSameRoomDirectoryAuthority(
  actual: RoomDirectoryAuthority,
  expected: RoomDirectoryAuthority
) {
  if (
    actual.server_id !== expected.server_id ||
    actual.authority_lineage_id !== expected.authority_lineage_id
  ) {
    throw new Error("방 목록 권위가 bootstrap 서버 및 계보와 일치하지 않습니다.");
  }
}

export function retainRoomDirectoryAuthority(
  actual: RoomDirectoryAuthority,
  pinned: RoomDirectoryAuthority | null,
  bootstrap: RoomDirectoryAuthority | null = null
): RoomDirectoryAuthority {
  if (pinned) assertSameRoomDirectoryAuthority(actual, pinned);
  if (bootstrap) assertSameRoomDirectoryAuthority(actual, bootstrap);
  return pinned ? { ...pinned } : { ...actual };
}

export async function bindRoomDirectoryAuthority(
  authority: RoomSessionSurface,
  trustedSurface: TrustedServerProductSurface | null = null,
  origin = window.location.origin
) {
  await assertServerProductSurfaceIntegrity(
    authority.server_product_surface,
    trustedSurface
  );
  bindVerifiedRoomDirectoryAuthority(authority, origin);
}

function bindVerifiedRoomDirectoryAuthority(
  authority: RoomSessionSurface,
  origin: string
) {
  if (boundAuthority?.origin === origin) {
    assertSameRoomDirectoryAuthority(authority, boundAuthority.authority);
    if (
      boundSurface?.origin !== origin ||
      boundSurface.surface.revision !== authority.server_product_surface.revision ||
      boundSurface.surface.digest !== authority.server_product_surface.digest
    ) {
      throw new Error("서버 제품 표면이 고정된 권위와 일치하지 않습니다.");
    }
    return;
  }
  boundAuthority = {
    origin,
    authority: {
      server_id: authority.server_id,
      authority_lineage_id: authority.authority_lineage_id,
    },
  };
  boundSurface = { origin, surface: structuredClone(authority.server_product_surface) };
}

export async function verifyAndBindRoomSessionSurface(
  authority: RoomSessionSurface,
  isCurrent: () => boolean,
  origin = window.location.origin
): Promise<boolean> {
  await assertServerProductSurfaceIntegrity(authority.server_product_surface, null);
  if (!isCurrent()) return false;
  bindVerifiedRoomDirectoryAuthority(authority, origin);
  return true;
}

export function currentRoomDirectoryAuthority(
  origin = window.location.origin
): RoomDirectoryAuthority | null {
  return boundAuthority?.origin === origin ? { ...boundAuthority.authority } : null;
}

export function currentServerProductSurface(
  origin = window.location.origin
): ServerProductSurface | null {
  return boundSurface?.origin === origin ? structuredClone(boundSurface.surface) : null;
}
