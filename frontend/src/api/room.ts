import type { RoomAppearance } from "../lib/roomAppearance";
import {
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
  postJsonHost,
  postJsonWithIdentity,
  postJsonWithToken,
  queryString,
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
  role: "human" | "director" | "implementer" | "reviewer" | "agent";
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

function normalizeRoomSettings(payload: ApiRoomSettings | undefined, fallbackRoomId: string): RoomSettings {
  const appearance = payload?.appearance || {};
  return {
    roomId: String(payload?.room_id || fallbackRoomId || ""),
    label: String(payload?.label || ""),
    topic: String(payload?.topic || ""),
    shortLabel: String(payload?.short_label || ""),
    appearance: {
      bannerPreset: appearance.banner_preset || "default",
      bannerImage: appearance.banner_image_url || undefined,
      iconImage: appearance.icon_image_url || undefined,
      iconLabel: appearance.icon_label || undefined,
      notifications: appearance.notifications || "mentions",
      inviteScope: appearance.invite_scope || "room",
    },
    channelSettings: normalizeChannelSettings(payload?.channel_settings),
    conversationMode: payload?.conversation_mode === "ambient" ? "ambient" : "ordered",
    toolMode: payload?.tool_mode === "tabletop" ? "tabletop" : "chat",
    orderedExcludePreviousSpeaker:
      payload?.ordered_exclude_previous_speaker !== false,
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

function roomAppearanceToApi(appearance: Partial<RoomAppearance> | undefined): ApiRoomAppearance {
  return {
    banner_preset: appearance?.bannerPreset,
    banner_image_url: appearance?.bannerImage,
    icon_image_url: appearance?.iconImage,
    icon_label: appearance?.iconLabel,
    notifications: appearance?.notifications,
    invite_scope: appearance?.inviteScope,
  };
}

function normalizeChannelSettings(
  payload: Record<string, ApiChannelSettings> | undefined
): Record<string, ChannelSettings> {
  if (!payload || typeof payload !== "object") return {};
  return Object.fromEntries(
    Object.entries(payload).map(([channelId, settings]) => [
      channelId,
      {
        notifications: settings?.notifications || "default",
        lastReadAt: settings?.last_read_at || undefined,
      },
    ])
  );
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
        last_read_at: value.lastReadAt,
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

type RoomSettingsUpdate = Partial<Omit<RoomSettings, "roomId" | "appearance">> & {
  roomId: string;
  appearance?: Partial<RoomAppearance>;
  identity?: RoomSettingsIdentity;
};

export function fetchRoomSettings(
  roomId: string,
  identity: RoomSettingsIdentity = {}
): Promise<RoomSettings> {
  return fetchJsonWithIdentity<{ room_id: string; settings: ApiRoomSettings }>(
    `/api/room-settings${queryString({ room_id: roomId })}`,
    identity
  ).then((payload) => normalizeRoomSettings(payload.settings, payload.room_id || roomId));
}

export function saveRoomSettings({
  roomId,
  label,
  topic,
  shortLabel,
  appearance,
  channelSettings,
  conversationMode,
  identity = {},
}: RoomSettingsUpdate): Promise<RoomSettings> {
  return postJsonWithIdentity<{ room_id: string; settings: ApiRoomSettings }>(
    "/api/room-settings",
    {
      room_id: roomId,
      label,
      topic,
      short_label: shortLabel,
      appearance: roomAppearanceToApi(appearance),
      channel_settings: channelSettingsToApi(channelSettings),
      conversation_mode: conversationMode,
    },
    identity
  ).then((payload) => normalizeRoomSettings(payload.settings, payload.room_id || roomId));
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

interface WsTicketResponse {
  ticket: string;
  ttl_seconds?: number;
}

export type RoomSocketTicket = string | DesktopRuntimeTicket;

export function getWsTicket(auth: RoomSocketAuth): Promise<RoomSocketTicket> {
  if (auth.kind === "host" && isDesktopWebview()) {
    return requestDesktopRuntimeTicket(auth.meetingId);
  }
  const body = auth.kind === "host" ? { meeting_id: auth.meetingId } : {};
  if (auth.kind === "host") {
    return postJsonHost<WsTicketResponse>("/api/ws-ticket", body).then((response) => response.ticket);
  }
  return postJsonWithToken<{ ticket: string; ttl_seconds?: number }>("/api/ws-ticket", body, auth.sessionToken).then(
    (response) => response.ticket
  );
}
