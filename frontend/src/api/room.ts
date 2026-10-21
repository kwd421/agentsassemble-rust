import { AGENT_CHAIN_LIMITS, agentChainLimitIsValid } from "../lib/agentChainLimit";
export { AGENT_CHAIN_LIMITS };
import { roomAttachmentPolicyIsValid } from "../lib/roomAttachmentPolicy";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";
import { fetchProductTransport, hasRemoteWorkspace, remoteSessionTransport, sessionHostOrigin } from "../lib/remote/remoteWorkspace";
import { connectionEnded } from "../lib/remote/remoteTransport";
import { isCustomChannelId } from "../lib/customChannelId";
import type { RoomAppearance } from "../lib/roomAppearance";
import type { Participant } from "../types/generated/Participant";
import {
  fetchDesktopRoomPreferences,
  isDesktopWebview,
  requestDesktopRuntimeTicket,
  type DesktopRuntimeTicket,
} from "../lib/desktopBridge";
import { parseBrowserRoomRuntimeTicket } from "../lib/roomRuntimeTicket";
import {
  fetchJson,
  fetchJsonServerOperator,
  fetchJsonWithIdentity,
  fetchJsonWithToken,
  exchangeSessionSocketTicket,
  postJsonServerOperator,
  postJsonWithIdentity,
  queryString,
  readBoundedJson,
  responseError,
} from "./http";
import {
  parseStrictRoomCreateResponse,
  parseStrictRoomDirectory,
} from "../lib/roomDirectoryContract";
import {
  assertExactKeys,
  strictRecord,
  stringField,
} from "../lib/strictJsonContract";
import {
  normalizeRoomChannelList,
  roomChannelListToApi,
  type ApiRoomChannel,
  type RoomChannel,
} from "./roomChannelCodec";

export type { RoomChannel } from "./roomChannelCodec";

export type ConversationMode = "ordered" | "ambient" | "free_access";
export type RoomToolMode = "chat" | "tabletop";

export type RoomGlobalAppearance = Omit<RoomAppearance, "notifications">;


export interface RoomGlobalSettings {
  agentChainLimit?: number;
  attachmentFileLimitBytes?: number;
  attachmentStorageLimitBytes?: number | null;
  lobbyName?: string;
  roomId: string;
  revision: string;
  label: string;
  topic: string;
  shortLabel: string;
  appearance: RoomGlobalAppearance;
  conversationMode: ConversationMode;
  toolMode: RoomToolMode;
  orderedExcludePreviousSpeaker: boolean;
  channels: RoomChannel[];
}

export type RoomGlobalSettingsUpdate = {
  agentChainLimit?: number;
  attachmentFileLimitBytes?: number;
  attachmentStorageLimitBytes?: number | null;
  label?: string;
  topic?: string;
  shortLabel?: string;
  appearance?: Partial<RoomGlobalAppearance>;
  conversationMode?: ConversationMode;
  toolMode?: RoomToolMode;
  orderedExcludePreviousSpeaker?: boolean;
  channels?: RoomChannel[];
};

export type ChannelNotificationSetting = "default" | "all" | "mentions" | "mute";

export type ChannelSettings = {
  notifications: ChannelNotificationSetting;
  lastReadAt?: string;
};

export interface RoomSettings {
  attachmentFileLimitBytes?: number;
  roomId: string;
  label: string;
  topic: string;
  shortLabel: string;
  appearance: RoomAppearance;
  channelSettings: Record<string, ChannelSettings>;
  conversationMode: ConversationMode;
  toolMode: RoomToolMode;
  orderedExcludePreviousSpeaker: boolean;
}

export interface ServerRoom {
  room_id: string;
  room_uid: string;
  label: string;
  last_active_at: string;
  archived: boolean;
  status?: "active" | "closed" | "archived" | string;
  origin: string;
  room_settings?: unknown;
}

export interface ServerRoomsResponse {
  server_id: string;
  authority_lineage_id: string;
  rooms: ServerRoom[];
}

export type RoomMember = Participant;

export interface VoiceParticipant {
  participantId: string;
  name: string;
  muted: boolean;
}

type ApiRoomAppearance = {
  banner_preset?: RoomAppearance["bannerPreset"];
  banner_image_url?: string;
  icon_image_url?: string;
  icon_label?: string;
  notifications?: RoomAppearance["notifications"];
  invite_scope?: RoomAppearance["inviteScope"];
};

type ApiRoomSettings = {
  agent_chain_limit?: number;
  attachment_file_limit_bytes?: number;
  attachment_storage_limit_bytes?: number | null;
  lobby_name?: string;
  room_id?: string;
  settings_revision?: string;
  label?: string;
  topic?: string;
  short_label?: string;
  appearance?: ApiRoomAppearance;
  channel_settings?: Record<string, ApiChannelSettings>;
  conversation_mode?: ConversationMode;
  tool_mode?: RoomToolMode;
  ordered_exclude_previous_speaker?: boolean;
  channels?: ApiRoomChannel[];
};

type ApiChannelSettings = {
  notifications?: ChannelNotificationSetting;
  last_read_at?: string;
};

function parseChannelSettings(
  value: unknown,
  label: string
): Record<string, ChannelSettings> {
  const settings = strictRecord(value, label);
  if (Object.keys(settings).length > 54) {
    throw new Error(`${label} 항목 수가 올바르지 않습니다.`);
  }
  return Object.fromEntries(
    Object.entries(settings).map(([channelId, raw]) => {
      if (!new Set(["lobby", "live", "board", "records"]).has(channelId) &&
        !isCustomChannelId(channelId)) {
        throw new Error(`${label}.${channelId} 식별자가 올바르지 않습니다.`);
      }
      const entry = strictRecord(raw, `${label}.${channelId}`);
      assertExactKeys(
        entry,
        ["notifications", "last_read_at"],
        `${label}.${channelId}`
      );
      const notifications = stringField(
        entry,
        "notifications",
        `${label}.${channelId}`
      );
      const lastReadAt = stringField(entry, "last_read_at", `${label}.${channelId}`);
      let cursorLength = 0;
      let cursorValid = true;
      for (const character of lastReadAt) {
        cursorLength += 1;
        if (cursorLength > 64 || character === "\r" || character === "\n" || character === "\t") {
          cursorValid = false;
          break;
        }
      }
      if (
        !new Set(["default", "all", "mentions", "mute"]).has(notifications) ||
        !cursorValid
      ) {
        throw new Error(`${label}.${channelId} 값이 올바르지 않습니다.`);
      }
      return [
        channelId,
        {
          notifications: notifications as ChannelNotificationSetting,
          lastReadAt: lastReadAt || undefined,
        },
      ];
    })
  );
}

function validateApiChannels(value: unknown, label: string) {
  if (!Array.isArray(value)) {
    throw new Error(`${label}가 배열이 아닙니다.`);
  }
  value.forEach((raw, index) => {
    const channel = strictRecord(raw, `${label}[${index}]`);
    assertExactKeys(
      channel,
      ["id", "name", "type", "position", "created_at"],
      `${label}[${index}]`
    );
    stringField(channel, "id", `${label}[${index}]`);
    stringField(channel, "name", `${label}[${index}]`);
    stringField(channel, "created_at", `${label}[${index}]`);
    if (
      !new Set(["text", "voice"]).has(
        stringField(channel, "type", `${label}[${index}]`)
      ) ||
      !Number.isSafeInteger(channel.position) ||
      Number(channel.position) < 0
    ) {
      throw new Error(`${label}[${index}] 값이 올바르지 않습니다.`);
    }
  });
}

function parseRoomSettingsResponse(value: unknown, expectedRoomId: string): RoomSettings {
  const response = strictRecord(value, "방 preference");
  assertExactKeys(response, ["room_id", "settings"], "방 preference");
  if (stringField(response, "room_id", "방 preference") !== expectedRoomId) {
    throw new Error("방 preference 응답의 방 권위가 일치하지 않습니다.");
  }
  const payload = strictRecord(response.settings, "방 preference.settings");
  assertExactKeys(
    payload,
    [
      "room_id",
      "settings_revision",
      "label",
      "topic",
      "short_label",
      "appearance",
      "channel_settings",
      "conversation_mode",
      "tool_mode",
      "ordered_exclude_previous_speaker",
      "channels",
    ],
    "방 preference.settings",
    ["lobby_name", "attachment_file_limit_bytes", "attachment_storage_limit_bytes", "agent_chain_limit"]
  );
  if (payload.agent_chain_limit !== undefined && !agentChainLimitIsValid(payload.agent_chain_limit)) {
    throw new Error("방 preference settings 값이 올바르지 않습니다.");
  }
  if (stringField(payload, "room_id", "방 preference.settings") !== expectedRoomId) {
    throw new Error("방 preference settings의 방 권위가 일치하지 않습니다.");
  }
  if (!roomAttachmentPolicyIsValid(payload)) throw new Error("방 첨부 파일 설정이 올바르지 않습니다.");
  const revision = stringField(payload, "settings_revision", "방 preference.settings");
  if (!/^room-settings-v1-[0-9a-f]{64}$/.test(revision)) {
    throw new Error("방 preference settings revision이 올바르지 않습니다.");
  }
  const appearance = strictRecord(payload.appearance, "방 preference.settings.appearance");
  assertExactKeys(
    appearance,
    [
      "banner_preset",
      "banner_image_url",
      "icon_image_url",
      "icon_label",
      "notifications",
      "invite_scope",
    ],
    "방 preference.settings.appearance"
  );
  const bannerPreset = stringField(
    appearance,
    "banner_preset",
    "방 preference.settings.appearance"
  );
  const notifications = stringField(
    appearance,
    "notifications",
    "방 preference.settings.appearance"
  );
  const inviteScope = stringField(
    appearance,
    "invite_scope",
    "방 preference.settings.appearance"
  );
  const conversationMode = stringField(
    payload,
    "conversation_mode",
    "방 preference.settings"
  );
  const toolMode = stringField(payload, "tool_mode", "방 preference.settings");
  validateApiChannels(payload.channels, "방 preference.settings.channels");
  if (
    !new Set(["default", "forest", "midnight", "ember", "custom"]).has(bannerPreset) ||
    !new Set(["all", "mentions", "mute"]).has(notifications) ||
    !new Set(["room", "read_only"]).has(inviteScope) ||
    !new Set(["ordered", "ambient", "free_access"]).has(conversationMode) ||
    !new Set(["chat", "tabletop"]).has(toolMode) ||
    (payload.lobby_name !== undefined && (typeof payload.lobby_name !== "string" || !payload.lobby_name.trim())) ||
    typeof payload.ordered_exclude_previous_speaker !== "boolean"
  ) {
    throw new Error("방 preference settings 값이 올바르지 않습니다.");
  }
  return {
    attachmentFileLimitBytes: payload.attachment_file_limit_bytes as number | undefined,
    roomId: expectedRoomId,
    label: stringField(payload, "label", "방 preference.settings"),
    topic: stringField(payload, "topic", "방 preference.settings"),
    shortLabel: stringField(payload, "short_label", "방 preference.settings"),
    appearance: {
      bannerPreset: bannerPreset as RoomAppearance["bannerPreset"],
      bannerImage:
        stringField(appearance, "banner_image_url", "방 preference.settings.appearance") ||
        undefined,
      iconImage:
        stringField(appearance, "icon_image_url", "방 preference.settings.appearance") ||
        undefined,
      iconLabel:
        stringField(appearance, "icon_label", "방 preference.settings.appearance") ||
        undefined,
      notifications: notifications as RoomAppearance["notifications"],
      inviteScope: inviteScope as RoomAppearance["inviteScope"],
    },
    channelSettings: parseChannelSettings(
      payload.channel_settings,
      "방 preference.settings.channel_settings"
    ),
    conversationMode: conversationMode as ConversationMode,
    toolMode: toolMode as RoomToolMode,
    orderedExcludePreviousSpeaker: payload.ordered_exclude_previous_speaker,
  };
}

export function normalizeRoomGlobalSettings(
  value: unknown,
  fallbackRoomId: string
): RoomGlobalSettings | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const payload = value as ApiRoomSettings;
  const appearance = payload.appearance;
  if (
    !roomAttachmentPolicyIsValid(payload) ||
    (payload.lobby_name !== undefined && (typeof payload.lobby_name !== "string" || !payload.lobby_name.trim())) ||
    typeof payload.settings_revision !== "string" ||
    !payload.settings_revision ||
    typeof payload.label !== "string" ||
    typeof payload.topic !== "string" ||
    !appearance ||
    typeof appearance !== "object" ||
    typeof appearance.banner_preset !== "string" ||
    typeof appearance.banner_image_url !== "string" ||
    typeof appearance.icon_image_url !== "string" ||
    typeof appearance.icon_label !== "string" ||
    typeof appearance.invite_scope !== "string" ||
    !["ordered", "ambient", "free_access"].includes(String(payload.conversation_mode || "")) ||
    !["chat", "tabletop"].includes(String(payload.tool_mode || "")) ||
    typeof payload.ordered_exclude_previous_speaker !== "boolean" ||
    !Array.isArray(payload.channels)
  ) {
    return null;
  }
  if (payload.agent_chain_limit !== undefined && !agentChainLimitIsValid(payload.agent_chain_limit)) return null;
  return {
    agentChainLimit: payload.agent_chain_limit ?? AGENT_CHAIN_LIMITS[0],
    attachmentFileLimitBytes: payload.attachment_file_limit_bytes ?? MAX_ATTACHMENT_BYTES,
    attachmentStorageLimitBytes: payload.attachment_storage_limit_bytes ?? null,
    lobbyName: payload.lobby_name,
    roomId: String(payload.room_id || fallbackRoomId || ""),
    revision: payload.settings_revision,
    label: payload.label,
    topic: payload.topic,
    shortLabel: appearance.icon_label,
    appearance: {
      bannerPreset: appearance.banner_preset,
      bannerImage: appearance.banner_image_url || undefined,
      iconImage: appearance.icon_image_url || undefined,
      iconLabel: appearance.icon_label || undefined,
      inviteScope: appearance.invite_scope,
    },
    conversationMode: payload.conversation_mode as ConversationMode,
    toolMode: payload.tool_mode as RoomToolMode,
    orderedExcludePreviousSpeaker: payload.ordered_exclude_previous_speaker,
    channels: normalizeRoomChannelList(payload.channels as ApiRoomChannel[]),
  };
}

export function roomGlobalSettingsUpdateToApi(
  updates: RoomGlobalSettingsUpdate
): Record<string, unknown> {
  const payload: Record<string, unknown> = {};
  if (updates.agentChainLimit !== undefined) payload.agent_chain_limit = updates.agentChainLimit;
  if (updates.attachmentFileLimitBytes !== undefined) payload.attachment_file_limit_bytes = updates.attachmentFileLimitBytes;
  if (updates.attachmentStorageLimitBytes !== undefined) payload.attachment_storage_limit_bytes = updates.attachmentStorageLimitBytes;
  if (updates.label !== undefined) payload.label = updates.label;
  if (updates.topic !== undefined) payload.topic = updates.topic;
  if (updates.conversationMode !== undefined) {
    payload.conversation_mode = updates.conversationMode;
  }
  if (updates.toolMode !== undefined) payload.tool_mode = updates.toolMode;
  if (updates.orderedExcludePreviousSpeaker !== undefined) {
    payload.ordered_exclude_previous_speaker =
      updates.orderedExcludePreviousSpeaker;
  }
  if (updates.channels !== undefined) {
    payload.channels = roomChannelListToApi(updates.channels);
  }
  const appearance: Record<string, unknown> = {};
  if (updates.appearance?.bannerPreset !== undefined) {
    appearance.banner_preset = updates.appearance.bannerPreset;
  }
  if (updates.appearance?.bannerImage !== undefined) {
    appearance.banner_image_url = updates.appearance.bannerImage;
  }
  if (updates.appearance?.iconImage !== undefined) {
    appearance.icon_image_url = updates.appearance.iconImage;
  }
  if (updates.appearance?.iconLabel !== undefined) {
    appearance.icon_label = updates.appearance.iconLabel;
  }
  if (updates.appearance?.inviteScope !== undefined) {
    appearance.invite_scope = updates.appearance.inviteScope;
  }
  if (updates.shortLabel !== undefined) appearance.icon_label = updates.shortLabel;
  if (Object.keys(appearance).length) payload.appearance = appearance;
  return payload;
}

function channelSettingsToApi(
  settings: Record<string, ChannelSettings> | undefined
): Record<string, ApiChannelSettings> | undefined {
  if (!settings) return undefined;
  return Object.fromEntries(
    Object.entries(settings).map(([channelId, value]) => [
      channelId,
      {
        notifications: value.notifications || "default",
        last_read_at: value.lastReadAt || "",
      },
    ])
  );
}

type RoomSettingsIdentity = {
  sessionToken?: string;
  deviceToken?: string;
};

export const ROOM_SESSION_PREFERENCES_UNAVAILABLE =
  "방 세션 인증이 완료되기 전에는 원격 알림 설정을 사용할 수 없습니다.";

type RoomSettingsUpdate = {
  roomId: string;
  roomUid: string;
  appearance?: Pick<RoomAppearance, "notifications">;
  channelSettings?: Record<string, ChannelSettings>;
  identity?: RoomSettingsIdentity;
};

function sessionHeaders(
  sessionToken: string,
  deviceToken: string | undefined,
  initHeaders: HeadersInit | undefined
): Headers {
  const headers = new Headers(initHeaders);
  headers.set("Authorization", `Bearer ${sessionToken}`);
  if (deviceToken) headers.set("X-Device-Token", deviceToken);
  return headers;
}

export function fetchRoomSettings(
  roomId: string,
  identity: RoomSettingsIdentity = {},
  signal?: AbortSignal
): Promise<RoomSettings> {
  const token = identity.sessionToken || "";
  // A presented session token always resolves through its own host binding
  // and fails closed without one (never the local origin): after a channel
  // loss or confirmed-loss unbind, a friend-room read must not run against
  // the local host with owner authority on the same room id, nor hit the
  // blocked `tauri://localhost` origin on desktop. Only calls with NO
  // session token take the desktop local-ticket path; the legacy plain
  // same-origin case (local web guest) lives inside
  // requestSessionRoomPreferences, which allows it only when no host is
  // live and the session was never bound.
  if (token) {
    return requestSessionRoomPreferences(
      token,
      identity.deviceToken,
      `/api/room-settings${queryString({ room_id: roomId })}`,
      { signal }
    ).then((payload) => parseRoomSettingsResponse(payload, roomId));
  }
  const request = isDesktopWebview()
    ? fetchDesktopRoomPreferences(roomId, { signal }, { kind: "local" }).then(async (response) => {
        if (!response.ok) throw await responseError(response);
        return response.json() as Promise<unknown>;
      })
    : fetchJsonWithIdentity<unknown>(
        `/api/room-settings${queryString({ room_id: roomId })}`,
        identity,
        signal
      );
  return request.then((payload) => parseRoomSettingsResponse(payload, roomId));
}

export function saveRoomSettings({
  roomId,
  roomUid,
  appearance,
  channelSettings,
  identity = {},
}: RoomSettingsUpdate): Promise<RoomSettings> {
  const body = {
    room_id: roomId,
    room_uid: roomUid,
    ...(appearance
      ? { appearance: { notifications: appearance.notifications } }
      : {}),
    ...(channelSettings
      ? { channel_settings: channelSettingsToApi(channelSettings) }
      : {}),
  };
  const token = identity.sessionToken || "";
  if (token) {
    return requestSessionRoomPreferences(
      token,
      identity.deviceToken,
      "/api/room-settings",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }
    ).then((payload) => parseRoomSettingsResponse(payload, roomId));
  }
  const request = isDesktopWebview()
    ? fetchDesktopRoomPreferences(roomId, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }, { kind: "local" }).then(async (response) => {
        if (!response.ok) throw await responseError(response);
        return response.json() as Promise<unknown>;
      })
    : postJsonWithIdentity<unknown>("/api/room-settings", body, identity);
  return request.then((payload) => parseRoomSettingsResponse(payload, roomId));
}

async function requestSessionRoomPreferences(
  sessionToken: string,
  deviceToken: string | undefined,
  url: string,
  init: RequestInit = {}
): Promise<unknown> {
  // A live host binding resolves through that host's own transport (absolute
  // origin). A dead binding fails closed: it must never degrade to a local
  // or relative fetch. The only legacy case left is a session the client
  // never bound (a local web guest), which keeps the exact previous
  // same-origin request. Desktop never reaches the legacy case: unbound
  // sessions resolve through desktop tickets above, and the desktop CSP would
  // block a relative fetch as `tauri://localhost`.
  const headers = sessionHeaders(sessionToken, deviceToken, init.headers);
  const transport = remoteSessionTransport(sessionToken);
  if (transport) {
    const response = await transport.fetch(url, { ...init, cache: "no-store", headers });
    if (!response.ok) throw await responseError(response);
    return readBoundedJson(response);
  }
  if (sessionHostOrigin(sessionToken) || isDesktopWebview()) throw connectionEnded();
  const response = await fetchProductTransport(url, { ...init, cache: "no-store", headers });
  if (!response.ok) throw await responseError(response);
  return readBoundedJson(response);
}

export function createRoom(
  requestId: string,
  roomId: string,
  label = "",
  beforeDispatch?: () => void
) {
  return postJsonServerOperator<unknown>("/api/rooms", {
    request_id: requestId,
    room_id: roomId,
    label,
  }, beforeDispatch).then(parseStrictRoomCreateResponse);
}

export function fetchRooms(includeArchived = false, beforeDispatch?: () => void, signal?: AbortSignal) {
  if (includeArchived) {
    return fetchJsonServerOperator<unknown>(
      "/api/rooms?include_archived=true",
      beforeDispatch,
      signal
    )
      .then(parseStrictRoomDirectory);
  }
  return fetchJsonServerOperator<unknown>("/api/rooms", beforeDispatch, signal)
    .then(parseStrictRoomDirectory);
}

export function fetchRoomChannels(meetingId: string, sessionToken = ""): Promise<RoomChannel[]> {
  const url = `/api/room-channels${queryString({ meeting_id: meetingId })}`;
  const result = sessionToken
    ? fetchJsonWithToken<{ channels: ApiRoomChannel[] }>(url, sessionToken)
    : fetchJson<{ channels: ApiRoomChannel[] }>(url);
  return result.then((payload) => normalizeRoomChannelList(payload.channels));
}

export type RoomSocketAuth =
  | { kind: "session"; sessionToken: string; deviceToken?: string }
  | { kind: "host"; meetingId: string };

export type RoomSocketTicket = DesktopRuntimeTicket;

export async function getWsTicket(auth: RoomSocketAuth): Promise<RoomSocketTicket> {
  if (auth.kind === "host" && isDesktopWebview()) {
    return requestDesktopRuntimeTicket(auth.meetingId, { kind: "local" });
  }
  if (auth.kind === "session") {
    // A session socket without a live binding must fail closed before any
    // local-origin request: never fall back to `tauri://localhost`, the
    // local host, or a relative same-origin fetch for a remote (friend)
    // room whose channel just died. Desktop has no plain fallback (its CSP
    // blocks the relative fetch); web keeps the exact legacy plain exchange
    // only when no host is live and the session was never bound.
    const transport = remoteSessionTransport(auth.sessionToken);
    if (!transport) {
      if (
        isDesktopWebview() ||
        hasRemoteWorkspace() ||
        sessionHostOrigin(auth.sessionToken)
      ) {
        throw connectionEnded();
      }
    }
    const payload = await exchangeSessionSocketTicket(auth.sessionToken, auth.deviceToken);
    const origin = transport?.hello.origin ?? window.location.href;
    return parseBrowserRoomRuntimeTicket(payload, origin);
  }
  throw new Error("Host WebSocket authority requires the desktop Rust runtime.");
}
