import type { VoiceParticipant } from "./room";
import type { RoomEvent as GeneratedRoomEvent } from "../types/generated/RoomEvent";
import {
  uploadMessageAttachment,
  type LobbyAttachmentRef,
} from "./messageAttachments";
import {
  fetchJson,
  fetchJsonWithToken,
  fileToBase64,
  postJson,
  postJsonWithIdentity,
  postJsonModerator,
  postJsonWithToken,
  queryString,
} from "./http";
export type { VoteSummary } from "../lib/roomVoteSummaryContract";

export type LobbyAttachmentUploadOptions = {
  roomId?: string;
  sessionToken?: string;
  deviceToken?: string;
  purpose?: "profile_avatar" | "room_appearance";
  signal?: AbortSignal;
  beforeDispatch?: () => void;
};

/** One agent that took its turn and chose not to speak, with the reason it gave. */
export interface LobbySkip {
  participant_id: string;
  name: string;
  reason: string;
  avatar_image_url?: string;
  provider_kind?: string;
}

export interface LobbyEvent {
  provider_request_id?: string;
  provider_request_title?: string;
  provider_request_state?: string;
  id: string;
  record_id?: string;
  reply_to_event_id?: string;
  seq?: number;
  kind: string;
  name: string;
  message: string;
  edited_at?: string;
  message_deleted?: boolean;
  side: string;
  created_at: string;
  official_record?: boolean;
  live_agent_endpoint?: boolean;
  actor_id?: string;
  actor_type?: string;
  avatar_image_url?: string;
  provider_kind?: string;
  role?: string;
  flow_id?: string;
  flow_meeting_id?: string;
  flow_event_type?: string;
  flow_action?: string;
  flow_reason?: string;
  target_event_id?: string;
  activity_id?: string;
  activity_title?: string;
  activity_detail?: string;
  activity_kind?: string;
  activity_category?: string;
  activity_status?: string;
  target_agent_id?: string;
  channel?: string;
  vote_id?: string;
  vote_question?: string;
  vote_options?: string[];
  vote_duration_seconds?: number;
  vote_deadline_at?: string;
  vote_choice?: string;
  attachments?: LobbyAttachmentRef[];
  skips?: LobbySkip[];
}

export interface LobbyPostResponse {
  event?: LobbyEvent;
  events: LobbyEvent[];
}

export type RoomEvent = GeneratedRoomEvent;

export function uploadLobbyAttachment(
  file: File,
  options: LobbyAttachmentUploadOptions = {}
): Promise<LobbyAttachmentRef> {
  const resolved = options;
  if (resolved.purpose === "profile_avatar" && !resolved.roomId && !resolved.sessionToken) {
    return Promise.reject(new Error("프로필 사진은 입장할 때 저장됩니다."));
  }
  if (!resolved.purpose) {
    return uploadMessageAttachment(
      file,
      resolved.roomId || "",
      resolved.sessionToken
        ? { kind: "remote", sessionToken: resolved.sessionToken, deviceToken: resolved.deviceToken }
        : { kind: "local" },
      resolved.beforeDispatch,
      resolved.signal
    );
  }
  return fileToBase64(file).then((dataBase64) => {
    const body = {
      purpose: resolved.purpose,
      filename: file.name || "attachment.bin",
      content_type: file.type || "application/octet-stream",
      data_base64: dataBase64,
    };
    if (resolved.purpose === "profile_avatar" && resolved.roomId) {
      return postJsonWithIdentity<{ attachment: LobbyAttachmentRef }>(
        "/api/attachments",
        body,
        { roomId: resolved.roomId, sessionToken: resolved.sessionToken, deviceToken: resolved.deviceToken }
      );
    }
    return postJsonModerator<{ attachment: LobbyAttachmentRef }>(
      "/api/attachments",
      body,
      resolved.sessionToken || ""
    );
  }).then((payload) => {
    return payload.attachment;
  });
}

export function fetchChannelLobby(
  channelId: string,
  options: { sessionToken?: string; meetingId?: string; after?: string } = {}
): Promise<LobbyEvent[]> {
  const token = options.sessionToken || "";
  const url = token
    ? `/api/room/channel-lobby${queryString({ channel_id: channelId, after: options.after })}`
    : `/api/room/channel-lobby${queryString({ channel_id: channelId, meeting_id: options.meetingId, after: options.after })}`;
  const result = token
    ? fetchJsonWithToken<{ events: LobbyEvent[] }>(url, token)
    : fetchJson<{ events: LobbyEvent[] }>(url);
  return result.then((payload) => payload.events || []);
}

export function postChannelSay(params: {
  channelId: string;
  message: string;
  sessionToken?: string;
  meetingId?: string;
  name?: string;
}): Promise<LobbyPostResponse> {
  return params.sessionToken
    ? postJsonWithToken<LobbyPostResponse>("/api/room/channel-say",
        { channel_id: params.channelId, message: params.message },
        params.sessionToken)
    : postJson<LobbyPostResponse>("/api/room/channel-say",
        { channel_id: params.channelId, message: params.message, meeting_id: params.meetingId, name: params.name });
}

type ApiVoiceParticipant = {
  participant_id?: string;
  name?: string;
  muted?: boolean;
};

function normalizeVoiceParticipants(participants: ApiVoiceParticipant[] | undefined): VoiceParticipant[] {
  return Array.isArray(participants)
    ? participants.map((p) => ({
        participantId: String(p.participant_id || ""),
        name: String(p.name || ""),
        muted: Boolean(p.muted),
      }))
    : [];
}

export function fetchVoicePresence(
  channelId: string,
  options: { sessionToken?: string; meetingId?: string } = {}
): Promise<VoiceParticipant[]> {
  const token = options.sessionToken || "";
  const url = token
    ? `/api/room/voice${queryString({ channel_id: channelId })}`
    : `/api/room/voice${queryString({ channel_id: channelId, meeting_id: options.meetingId })}`;
  const result = token
    ? fetchJsonWithToken<{ participants: ApiVoiceParticipant[] }>(url, token)
    : fetchJson<{ participants: ApiVoiceParticipant[] }>(url);
  return result.then((payload) => normalizeVoiceParticipants(payload.participants));
}

export function joinVoiceChannel(params: {
  channelId: string;
  sessionToken?: string;
  meetingId?: string;
  name?: string;
  muted?: boolean;
}): Promise<VoiceParticipant[]> {
  const result = params.sessionToken
    ? postJsonWithToken<{ participants: ApiVoiceParticipant[] }>("/api/room/voice/join",
        { channel_id: params.channelId, muted: Boolean(params.muted) }, params.sessionToken)
    : postJson<{ participants: ApiVoiceParticipant[] }>("/api/room/voice/join",
        { channel_id: params.channelId, muted: Boolean(params.muted), meeting_id: params.meetingId, name: params.name });
  return result.then((payload) => normalizeVoiceParticipants(payload.participants));
}

export function leaveVoiceChannel(params: {
  channelId: string;
  sessionToken?: string;
  meetingId?: string;
}): Promise<VoiceParticipant[]> {
  const result = params.sessionToken
    ? postJsonWithToken<{ participants: ApiVoiceParticipant[] }>("/api/room/voice/leave",
        { channel_id: params.channelId }, params.sessionToken)
    : postJson<{ participants: ApiVoiceParticipant[] }>("/api/room/voice/leave",
        { channel_id: params.channelId, meeting_id: params.meetingId });
  return result.then((payload) => normalizeVoiceParticipants(payload.participants));
}

export function lobbyEventId(event: LobbyEvent): string {
  return String(
    event.id ||
      [event.name, event.kind, event.created_at, event.message]
        .filter(Boolean)
        .join(":")
  ).trim();
}

function isMessageMutationTransition(event: LobbyEvent) {
  return Boolean(
    event.target_event_id &&
    ["message_updated", "message_deleted"].includes(event.flow_action || ""),
  );
}

export function mergeLobbyEvents(
  existing: LobbyEvent[],
  incoming: LobbyEvent[]
): LobbyEvent[] {
  const byId = new Map<string, LobbyEvent>();
  const byRecordId = new Map<string, string>();
  const latestMutationByTarget = new Map<string, LobbyEvent>();
  const order: string[] = [];
  incoming.forEach((event) => {
    if (isMessageMutationTransition(event)) {
      latestMutationByTarget.set(event.target_event_id || "", event);
    }
  });
  for (const event of existing) {
    const eventId = lobbyEventId(event);
    if (!eventId) continue;
    byId.set(eventId, event);
    if (event.record_id) byRecordId.set(event.record_id, eventId);
    order.push(eventId);
  }
  for (const event of incoming) {
    if (isMessageMutationTransition(event)) {
      const targetEventId = event.target_event_id || "";
      if (latestMutationByTarget.get(targetEventId) !== event) continue;
      const targetId = byRecordId.get(targetEventId);
      const target = targetId ? byId.get(targetId) : undefined;
      if (!targetId || !target) continue;
      const replacement = event.flow_action === "message_deleted"
        ? target.message_deleted === true &&
          target.message === "삭제된 메시지입니다" &&
          target.attachments?.length === 0
          ? target
          : {
              ...target,
              message: "삭제된 메시지입니다",
              attachments: [],
              message_deleted: true,
            }
        : target.message === event.message && target.edited_at === event.edited_at
          ? target
          : {
              ...target,
              message: event.message,
              edited_at: event.edited_at,
            };
      byId.set(
        targetId,
        replacement,
      );
      continue;
    }
    const eventId = lobbyEventId(event);
    if (!eventId) continue;
    if (!byId.has(eventId)) order.push(eventId);
    byId.set(eventId, event);
    if (event.record_id) byRecordId.set(event.record_id, eventId);
  }
  return order.map((eventId) => byId.get(eventId)).filter(Boolean) as LobbyEvent[];
}

export function mergeLobbyEventsByCreatedAt(
  existing: LobbyEvent[],
  incoming: LobbyEvent[]
): LobbyEvent[] {
  return mergeLobbyEvents(existing, incoming)
    .slice()
    .sort((left, right) => left.created_at.localeCompare(right.created_at));
}

export function parseLobbyStreamData(raw: string): LobbyEvent[] {
  try {
    const data = JSON.parse(raw) as { stream?: string; events?: unknown[] } | LobbyEvent | null;
    if (!data || typeof data !== "object") return [];
    if ("stream" in data && data.stream && data.stream !== "lobby") return [];
    if ("events" in data && Array.isArray(data.events)) {
      return data.events.filter(isLobbyEvent) as LobbyEvent[];
    }
    return isLobbyEvent(data) ? [data] : [];
  } catch {
    return [];
  }
}

function isLobbyEvent(value: unknown): value is LobbyEvent {
  if (!value || typeof value !== "object") return false;
  const event = value as Partial<LobbyEvent> & { channel?: unknown };
  if (typeof event.channel === "string" && event.channel !== "lobby") return false;
  return typeof event.id === "string" && typeof event.name === "string";
}
