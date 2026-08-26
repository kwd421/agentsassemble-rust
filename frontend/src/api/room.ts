import type { RoomAppearance } from "../lib/roomAppearance";
import type { ParticipantRole } from "../types/generated/ParticipantRole";
import {
  fetchDesktopRoomPreferences,
  isDesktopWebview,
  requestDesktopRuntimeTicket,
  type DesktopRuntimeTicket,
} from "../lib/desktopBridge";
import {
  fetchJson,
  fetchJsonServerOperator,
  fetchJsonWithIdentity,
  fetchJsonWithToken,
  deleteJson,
  postJson,
  postJsonServerOperator,
  postJsonWithIdentity,
  queryString,
  responseError,
} from "./http";
import {
  parseStrictRoomCreateResponse,
  parseStrictRoomDirectory,
} from "../lib/roomDirectoryContract";
import {
  normalizeRoomChannelList,
  roomChannelListToApi,
  type ApiRoomChannel,
  type RoomChannel,
} from "./roomChannelCodec";

export type { RoomChannel } from "./roomChannelCodec";

export type ConversationMode = "ordered" | "ambient";
export type RoomToolMode = "chat" | "tabletop";

export type RoomGlobalAppearance = Omit<RoomAppearance, "notifications">;

export interface RoomGlobalSettings {
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
  activityPlugin?: string;
}

export type RoomGlobalSettingsUpdate = {
  label?: string;
  topic?: string;
  shortLabel?: string;
  appearance?: Partial<RoomGlobalAppearance>;
  conversationMode?: ConversationMode;
  toolMode?: RoomToolMode;
  orderedExcludePreviousSpeaker?: boolean;
  channels?: RoomChannel[];
  activityPlugin?: string;
};

export type ChannelNotificationSetting = "default" | "all" | "mentions" | "mute";

export type ChannelSettings = {
  notifications: ChannelNotificationSetting;
  lastReadAt?: string;
};

export interface RoomSettings {
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

export type ParticipantType = "human" | "subscription_ai" | "api" | "local" | "remote" | "unknown";

export interface RoomFriend {
  friend_id: string;
  display_name: string;
  handle: string;
  participant_type: ParticipantType;
  provider_kind: string;
  connection_kind: string;
  external_owned?: boolean;
  agent_id?: string;
  source_agent_id: string;
  last_meeting_id: string;
  status: string;
  source: string;
  created_at: string;
  updated_at: string;
  last_seen_at?: string;
}

export interface RoomMember {
  meeting_id: string;
  participant_id: string;
  display_name: string;
  avatar_image_url?: string;
  role: ParticipantRole;
  participant_type: ParticipantType;
  provider_kind: string;
  connection_kind: string;
  session_id?: string;
  owner_id?: string;
  created_by?: string;
  model_id?: string;
  effort?: string;
  sandbox_enforcement?: string;
  permission_option?: string;
  runtime_sharing_policy?: string;
  engagement_mode?: string;
  execution_mode?: string;
  join_semantics?: string;
  session_status?: string;
  thinking?: boolean;
  status: string;
  muted?: boolean;
  source: string;
  created_at: string;
  updated_at: string;
  last_seen_at?: string;
}

export interface RoomFriendsResponse {
  friends: RoomFriend[];
  candidates: RoomFriend[];
}

export interface UserProfile {
  displayName: string;
  handle: string;
  status: "online" | "idle" | "dnd" | "offline";
  customStatus: string;
  avatarLabel: string;
  avatarImage?: string;
  bannerPreset: "default" | "forest" | "midnight" | "ember" | "custom";
  accentColor: string;
  micMuted: boolean;
  deafened: boolean;
  createdAt?: string;
  updatedAt?: string;
}

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
  activity_plugin?: string;
};

type ApiChannelSettings = {
  notifications?: ChannelNotificationSetting;
  last_read_at?: string;
};

type ApiUserProfile = {
  revision?: number;
  display_name?: string;
  handle?: string;
  status?: UserProfile["status"];
  custom_status?: string;
  avatar_label?: string;
  avatar_image_url?: string;
  banner_preset?: UserProfile["bannerPreset"];
  accent_color?: string;
  mic_muted?: boolean;
  deafened?: boolean;
  created_at?: string;
  updated_at?: string;
};

function strictRecord(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 응답 형식이 올바르지 않습니다.`);
  }
  return value as Record<string, unknown>;
}

function requireExactKeys(
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

function requiredApiString(value: Record<string, unknown>, key: string, label: string): string {
  if (typeof value[key] !== "string") {
    throw new Error(`${label}.${key}가 올바르지 않습니다.`);
  }
  return value[key] as string;
}

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
        !/^c[0-9a-f]{12}$/.test(channelId)) {
        throw new Error(`${label}.${channelId} 식별자가 올바르지 않습니다.`);
      }
      const entry = strictRecord(raw, `${label}.${channelId}`);
      requireExactKeys(
        entry,
        ["notifications", "last_read_at"],
        `${label}.${channelId}`
      );
      const notifications = requiredApiString(
        entry,
        "notifications",
        `${label}.${channelId}`
      );
      const lastReadAt = requiredApiString(entry, "last_read_at", `${label}.${channelId}`);
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
    requireExactKeys(
      channel,
      ["id", "name", "type", "position", "created_at"],
      `${label}[${index}]`
    );
    requiredApiString(channel, "id", `${label}[${index}]`);
    requiredApiString(channel, "name", `${label}[${index}]`);
    requiredApiString(channel, "created_at", `${label}[${index}]`);
    if (
      !new Set(["text", "voice"]).has(
        requiredApiString(channel, "type", `${label}[${index}]`)
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
  requireExactKeys(response, ["room_id", "settings"], "방 preference");
  if (requiredApiString(response, "room_id", "방 preference") !== expectedRoomId) {
    throw new Error("방 preference 응답의 방 권위가 일치하지 않습니다.");
  }
  const payload = strictRecord(response.settings, "방 preference.settings");
  requireExactKeys(
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
      "activity_plugin",
    ],
    "방 preference.settings"
  );
  if (requiredApiString(payload, "room_id", "방 preference.settings") !== expectedRoomId) {
    throw new Error("방 preference settings의 방 권위가 일치하지 않습니다.");
  }
  const revision = requiredApiString(payload, "settings_revision", "방 preference.settings");
  if (!/^room-settings-v1-[0-9a-f]{64}$/.test(revision)) {
    throw new Error("방 preference settings revision이 올바르지 않습니다.");
  }
  const appearance = strictRecord(payload.appearance, "방 preference.settings.appearance");
  requireExactKeys(
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
  const bannerPreset = requiredApiString(
    appearance,
    "banner_preset",
    "방 preference.settings.appearance"
  );
  const notifications = requiredApiString(
    appearance,
    "notifications",
    "방 preference.settings.appearance"
  );
  const inviteScope = requiredApiString(
    appearance,
    "invite_scope",
    "방 preference.settings.appearance"
  );
  const conversationMode = requiredApiString(
    payload,
    "conversation_mode",
    "방 preference.settings"
  );
  const toolMode = requiredApiString(payload, "tool_mode", "방 preference.settings");
  requiredApiString(payload, "activity_plugin", "방 preference.settings");
  validateApiChannels(payload.channels, "방 preference.settings.channels");
  if (
    !new Set(["default", "forest", "midnight", "ember", "custom"]).has(bannerPreset) ||
    !new Set(["all", "mentions", "mute"]).has(notifications) ||
    !new Set(["room", "read_only"]).has(inviteScope) ||
    !new Set(["ordered", "ambient"]).has(conversationMode) ||
    !new Set(["chat", "tabletop"]).has(toolMode) ||
    typeof payload.ordered_exclude_previous_speaker !== "boolean"
  ) {
    throw new Error("방 preference settings 값이 올바르지 않습니다.");
  }
  return {
    roomId: expectedRoomId,
    label: requiredApiString(payload, "label", "방 preference.settings"),
    topic: requiredApiString(payload, "topic", "방 preference.settings"),
    shortLabel: requiredApiString(payload, "short_label", "방 preference.settings"),
    appearance: {
      bannerPreset: bannerPreset as RoomAppearance["bannerPreset"],
      bannerImage:
        requiredApiString(appearance, "banner_image_url", "방 preference.settings.appearance") ||
        undefined,
      iconImage:
        requiredApiString(appearance, "icon_image_url", "방 preference.settings.appearance") ||
        undefined,
      iconLabel:
        requiredApiString(appearance, "icon_label", "방 preference.settings.appearance") ||
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
    !["ordered", "ambient"].includes(String(payload.conversation_mode || "")) ||
    !["chat", "tabletop"].includes(String(payload.tool_mode || "")) ||
    typeof payload.ordered_exclude_previous_speaker !== "boolean" ||
    !Array.isArray(payload.channels)
  ) {
    return null;
  }
  return {
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
    activityPlugin: String(payload.activity_plugin || ""),
  };
}

export function roomGlobalSettingsUpdateToApi(
  updates: RoomGlobalSettingsUpdate
): Record<string, unknown> {
  const payload: Record<string, unknown> = {};
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

function normalizeUserProfile(payload: ApiUserProfile | undefined): UserProfile {
  if (
    !payload ||
    !Number.isInteger(payload.revision) ||
    Number(payload.revision) < 1 ||
    typeof payload.display_name !== "string" ||
    typeof payload.handle !== "string" ||
    !["online", "idle", "dnd", "offline"].includes(String(payload.status || "")) ||
    typeof payload.custom_status !== "string" ||
    typeof payload.avatar_label !== "string" ||
    typeof payload.avatar_image_url !== "string" ||
    !["default", "forest", "midnight", "ember", "custom"].includes(
      String(payload.banner_preset || "")
    ) ||
    typeof payload.accent_color !== "string" ||
    typeof payload.mic_muted !== "boolean" ||
    typeof payload.deafened !== "boolean" ||
    typeof payload.created_at !== "string" ||
    typeof payload.updated_at !== "string"
  ) {
    throw new Error("서버 사용자 프로필 응답이 현재 계약과 일치하지 않습니다.");
  }
  return {
    displayName: payload.display_name,
    handle: payload.handle,
    status: payload.status as UserProfile["status"],
    customStatus: payload.custom_status,
    avatarLabel: payload.avatar_label,
    avatarImage: payload.avatar_image_url || undefined,
    bannerPreset: payload.banner_preset as UserProfile["bannerPreset"],
    accentColor: payload.accent_color,
    micMuted: payload.mic_muted,
    deafened: payload.deafened,
    createdAt: payload.created_at,
    updatedAt: payload.updated_at,
  };
}

function userProfileToApi(profile: UserProfile): ApiUserProfile {
  return {
    display_name: profile.displayName,
    handle: profile.handle,
    status: profile.status,
    custom_status: profile.customStatus,
    avatar_label: profile.avatarLabel,
    avatar_image_url: profile.avatarImage,
    banner_preset: profile.bannerPreset,
    accent_color: profile.accentColor,
    mic_muted: profile.micMuted,
    deafened: profile.deafened,
  };
}

type RoomSettingsIdentity = {
  sessionToken?: string;
  deviceToken?: string;
};

type RoomSettingsUpdate = {
  roomId: string;
  appearance?: Pick<RoomAppearance, "notifications">;
  channelSettings?: Record<string, ChannelSettings>;
  identity?: RoomSettingsIdentity;
};

export function fetchRoomSettings(
  roomId: string,
  identity: RoomSettingsIdentity = {}
): Promise<RoomSettings> {
  const request =
    !identity.sessionToken && isDesktopWebview()
      ? fetchDesktopRoomPreferences(roomId).then(async (response) => {
          if (!response.ok) throw await responseError(response);
          return response.json() as Promise<unknown>;
        })
      : fetchJsonWithIdentity<unknown>(
          `/api/room-settings${queryString({ room_id: roomId })}`,
          identity
        );
  return request.then((payload) => parseRoomSettingsResponse(payload, roomId));
}

export function saveRoomSettings({
  roomId,
  appearance,
  channelSettings,
  identity = {},
}: RoomSettingsUpdate): Promise<RoomSettings> {
  const body = {
    room_id: roomId,
    ...(appearance
      ? { appearance: { notifications: appearance.notifications } }
      : {}),
    ...(channelSettings
      ? { channel_settings: channelSettingsToApi(channelSettings) }
      : {}),
  };
  const request =
    !identity.sessionToken && isDesktopWebview()
      ? fetchDesktopRoomPreferences(roomId, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        }).then(async (response) => {
          if (!response.ok) throw await responseError(response);
          return response.json() as Promise<unknown>;
        })
      : postJsonWithIdentity<unknown>("/api/room-settings", body, identity);
  return request.then((payload) => parseRoomSettingsResponse(payload, roomId));
}

export function fetchRoomFriends() {
  return fetchJson<RoomFriendsResponse>("/api/room-friends");
}

export function createRoom(requestId: string, roomId: string, label = "") {
  return postJsonServerOperator<unknown>("/api/rooms", {
    request_id: requestId,
    room_id: roomId,
    label,
  }).then(parseStrictRoomCreateResponse);
}

export function fetchRooms(includeArchived = false) {
  if (includeArchived) {
    return fetchJsonServerOperator<unknown>("/api/rooms?include_archived=true")
      .then(parseStrictRoomDirectory);
  }
  return fetchJsonServerOperator<unknown>("/api/rooms").then(parseStrictRoomDirectory);
}

export function addRoomFriend(friend: Partial<RoomFriend>) {
  return postJson<{ friend: RoomFriend; friends: RoomFriend[] }>("/api/room-friends", friend);
}

export function deleteRoomFriend(friendId: string) {
  return deleteJson<RoomFriendsResponse & { deleted: { friend_id: string } }>(
    `/api/room-friends${queryString({ friend_id: friendId })}`
  );
}

export type UserProfileIdentity = {
  sessionToken?: string;
  deviceToken?: string;
  roomId?: string;
};

export function fetchUserProfile(identity: UserProfileIdentity = {}): Promise<UserProfile> {
  return fetchJsonWithIdentity<{ profile: ApiUserProfile }>("/api/user-profile", identity).then((payload) =>
    normalizeUserProfile(payload.profile)
  );
}

export function saveUserProfile(
  profile: UserProfile,
  identity: UserProfileIdentity = {}
): Promise<UserProfile> {
  return postJsonWithIdentity<{ profile: ApiUserProfile }>(
    "/api/user-profile",
    userProfileToApi(profile),
    identity
  ).then(
    (payload) => normalizeUserProfile(payload.profile)
  );
}

export function fetchRoomChannels(meetingId: string, sessionToken = ""): Promise<RoomChannel[]> {
  const url = `/api/room-channels${queryString({ meeting_id: meetingId })}`;
  const result = sessionToken
    ? fetchJsonWithToken<{ channels: ApiRoomChannel[] }>(url, sessionToken)
    : fetchJson<{ channels: ApiRoomChannel[] }>(url);
  return result.then((payload) => normalizeRoomChannelList(payload.channels));
}

export type RoomSocketAuth =
  | { kind: "session"; sessionToken: string }
  | { kind: "host"; meetingId: string };

export type RoomSocketTicket = DesktopRuntimeTicket;

export function getWsTicket(auth: RoomSocketAuth): Promise<RoomSocketTicket> {
  if (auth.kind === "host" && isDesktopWebview()) {
    return requestDesktopRuntimeTicket(auth.meetingId);
  }
  return Promise.reject(
    new Error(
      "Central and guest WebSocket ticket authority is not implemented by the Rust product surface."
    )
  );
}
