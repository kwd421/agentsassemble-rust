import type { RoomAppearance } from "./roomAppearance";
import {
  parseRoomSessionSurface,
  type RoomSessionSurface,
} from "./roomDirectoryContract";
import type {
  GuestRecoveryRedeemResponse,
  OperatorPairingRedeemResponse,
  RoomInviteJoinResponse,
} from "./roomAdmissionContract";
import {
  assertExactKeys,
  optionalString,
  requiredString,
  strictRecord,
  stringField,
} from "./strictJsonContract";

export type RoomGuestSession = {
  inviteToken: string;
  sessionToken: string;
  meetingId: string;
  agentId: string;
  displayName: string;
  avatarImage?: string;
  inviteScope: RoomAppearance["inviteScope"];
  expiresAt: string;
  joinedAt: string;
  roomLabel?: string;
  roomTopic?: string;
  roomCreatedAt?: string;
  roomUid?: string;
  clientId?: string;
  serverSurface: RoomSessionSurface;
  // True when this session belongs to the server operator's account —
  // unlocks host moderation through the public entrance.
  operator?: boolean;
};

const ROOM_GUEST_SESSION_STORAGE_KEY = "agentsassemble.roomGuestSession.v1";
const ROOM_GUEST_SESSION_STORAGE_UNAVAILABLE =
  "방 세션을 브라우저에 영구 저장할 수 없습니다. 저장 공간 또는 사이트 설정을 확인한 뒤 다시 시도해 주세요.";

function cleanText(value: unknown, limit: number): string {
  return String(value || "")
    .replace(/[\r\n\t]/g, " ")
    .trim()
    .slice(0, limit)
    .trim();
}

export function joinInviteTokenFromUrl(url: string): string {
  try {
    const parsed = new URL(url);
    const joinPath = parsed.pathname.replace(/\/+$/, "") || "/";
    if (joinPath !== "/join") return "";
    return cleanText(parsed.searchParams.get("token"), 4096);
  } catch {
    return "";
  }
}

export function operatorPairingTokenFromUrl(url: string): string {
  try {
    const parsed = new URL(url);
    const pairPath = parsed.pathname.replace(/\/+$/, "") || "/";
    if (pairPath !== "/pair") return "";
    return cleanText(parsed.searchParams.get("token"), 4096);
  } catch {
    return "";
  }
}

export function consumeOperatorPairingTokenFromUrl(): string {
  const token = operatorPairingTokenFromUrl(window.location.href);
  if (!token) return "";
  window.history.replaceState({}, "", window.location.pathname || "/pair");
  return token;
}

export function roomGuestSessionFromJoinPayload(
  inviteToken: string,
  payload: RoomInviteJoinResponse,
  now = new Date()
): RoomGuestSession {
  return roomGuestSessionFromAdmissionPayload(inviteToken, payload, now);
}

export function roomGuestSessionFromPairingPayload(
  payload: OperatorPairingRedeemResponse,
  now = new Date()
): RoomGuestSession {
  return roomGuestSessionFromAdmissionPayload("", payload, now);
}

export function roomGuestSessionFromRecoveryPayload(
  payload: GuestRecoveryRedeemResponse,
  now = new Date()
): RoomGuestSession {
  return roomGuestSessionFromAdmissionPayload("", payload, now);
}

function roomGuestSessionFromAdmissionPayload(
  inviteToken: string,
  payload:
    | RoomInviteJoinResponse
    | OperatorPairingRedeemResponse
    | GuestRecoveryRedeemResponse,
  now: Date
): RoomGuestSession {
  return {
    inviteToken,
    sessionToken: payload.session_token,
    meetingId: payload.meeting_id,
    agentId: payload.agent_id,
    displayName: payload.display_name,
    avatarImage: "avatar_image_url" in payload ? payload.avatar_image_url : undefined,
    inviteScope: payload.invite_scope,
    expiresAt: payload.expires_at,
    joinedAt: "joined_at" in payload ? payload.joined_at : now.toISOString(),
    roomLabel: payload.room_label || undefined,
    roomTopic: payload.room_topic || undefined,
    roomCreatedAt: payload.room_created_at || undefined,
    roomUid: "room_uid" in payload ? payload.room_uid : undefined,
    clientId: "client_id" in payload ? payload.client_id : undefined,
    serverSurface: {
      server_id: payload.server_id,
      authority_lineage_id: payload.authority_lineage_id,
      server_product_surface: payload.server_product_surface,
    },
    operator: "operator" in payload && payload.operator,
  };
}

export function normalizeRoomGuestSession(value: unknown): RoomGuestSession | null {
  try {
    const record = strictRecord(value, "저장된 방 세션");
    assertExactKeys(
      record,
      [
        "inviteToken",
        "sessionToken",
        "meetingId",
        "agentId",
        "displayName",
        "inviteScope",
        "expiresAt",
        "joinedAt",
        "serverSurface",
        "operator",
      ],
      "저장된 방 세션",
      ["avatarImage", "roomLabel", "roomTopic", "roomCreatedAt", "roomUid", "clientId"]
    );
    if (
      (record.inviteScope !== "room" && record.inviteScope !== "read_only") ||
      typeof record.operator !== "boolean"
    ) {
      return null;
    }
    const expiresAt = requiredString(record, "expiresAt", "저장된 방 세션");
    const joinedAt = requiredString(record, "joinedAt", "저장된 방 세션");
    if (Number.isNaN(Date.parse(expiresAt)) || Number.isNaN(Date.parse(joinedAt))) {
      return null;
    }
    return {
      inviteToken: stringField(record, "inviteToken", "저장된 방 세션"),
      sessionToken: requiredString(record, "sessionToken", "저장된 방 세션"),
      meetingId: requiredString(record, "meetingId", "저장된 방 세션"),
      agentId: requiredString(record, "agentId", "저장된 방 세션"),
      displayName: requiredString(record, "displayName", "저장된 방 세션"),
      avatarImage: optionalString(record, "avatarImage", "저장된 방 세션"),
      inviteScope: record.inviteScope,
      expiresAt,
      joinedAt,
      roomLabel: optionalString(record, "roomLabel", "저장된 방 세션"),
      roomTopic: optionalString(record, "roomTopic", "저장된 방 세션"),
      roomCreatedAt: optionalString(record, "roomCreatedAt", "저장된 방 세션"),
      roomUid: optionalString(record, "roomUid", "저장된 방 세션"),
      clientId: optionalString(record, "clientId", "저장된 방 세션"),
      serverSurface: parseRoomSessionSurface(record.serverSurface),
      operator: record.operator,
    };
  } catch {
    return null;
  }
}

// A guest session token lives ~1h server-side. Treat it as expired a minute
// early so we re-join (reusable invite + device token = stable identity)
// instead of firing a doomed request with a token about to die.
const GUEST_SESSION_EXPIRY_SKEW_MS = 60_000;

export function roomGuestSessionExpired(
  session: RoomGuestSession | null | undefined,
  now: number = Date.now()
): boolean {
  if (!session) return true;
  const expiresAt = Date.parse(session.expiresAt || "");
  if (Number.isNaN(expiresAt)) return true;
  return expiresAt - GUEST_SESSION_EXPIRY_SKEW_MS <= now;
}

export function loadRoomGuestSession(): RoomGuestSession | null {
  try {
    const raw = window.localStorage.getItem(ROOM_GUEST_SESSION_STORAGE_KEY);
    return normalizeRoomGuestSession(raw ? JSON.parse(raw) : null);
  } catch {
    return null;
  }
}

export function persistRoomGuestSession(session: RoomGuestSession | null) {
  if (session) {
    try {
      const serialized = JSON.stringify(session);
      window.localStorage.setItem(ROOM_GUEST_SESSION_STORAGE_KEY, serialized);
      const stored = window.localStorage.getItem(ROOM_GUEST_SESSION_STORAGE_KEY);
      if (
        stored !== serialized ||
        !normalizeRoomGuestSession(JSON.parse(stored))
      ) {
        throw new Error(ROOM_GUEST_SESSION_STORAGE_UNAVAILABLE);
      }
      return;
    } catch {
      throw new Error(ROOM_GUEST_SESSION_STORAGE_UNAVAILABLE);
    }
  }
  try {
    window.localStorage.removeItem(ROOM_GUEST_SESSION_STORAGE_KEY);
  } catch {
    // Server-side expiry/revocation remains authoritative when local cleanup fails.
  }
}
