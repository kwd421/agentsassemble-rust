import type { ServerRoomDockSource } from "./roomDockModel";

export type StrictRoomDirectory = {
  server_id: string;
  authority_lineage_id: string;
  rooms: ServerRoomDockSource[];
};

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function record(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 응답 형식이 올바르지 않습니다.`);
  }
  return value as Record<string, unknown>;
}

function exactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
  label: string
) {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (
    actual.length !== canonical.length ||
    actual.some((key, index) => key !== canonical[index])
  ) {
    throw new Error(`${label} 응답 계약이 일치하지 않습니다.`);
  }
}

function requiredString(value: Record<string, unknown>, key: string, label: string) {
  if (typeof value[key] !== "string" || !value[key]) {
    throw new Error(`${label}.${key}가 올바르지 않습니다.`);
  }
  return value[key] as string;
}

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

export function parseStrictRoomDirectory(value: unknown): StrictRoomDirectory {
  const payload = record(value, "방 목록");
  exactKeys(payload, ["server_id", "authority_lineage_id", "rooms"], "방 목록");
  const serverId = requiredString(payload, "server_id", "방 목록");
  const lineageId = requiredString(payload, "authority_lineage_id", "방 목록");
  if (!UUID_PATTERN.test(serverId) || !UUID_PATTERN.test(lineageId)) {
    throw new Error("방 목록의 서버 또는 권위 계보 식별자가 UUID가 아닙니다.");
  }
  if (!Array.isArray(payload.rooms)) {
    throw new Error("방 목록 rooms가 배열이 아닙니다.");
  }
  return {
    server_id: serverId,
    authority_lineage_id: lineageId,
    rooms: payload.rooms.map(validateRoom),
  };
}
