import type { RoomAppearance } from "./roomAppearance";
import {
  parseRoomSessionSurface,
  type RoomSessionSurface,
} from "./roomDirectoryContract";
import {
  assertExactKeys,
  optionalString,
  requiredString,
  strictRecord,
  stringField,
} from "./strictJsonContract";

type AdmissionSessionBase = RoomSessionSurface & {
  session_token: string;
  agent_id: string;
  display_name: string;
  meeting_id: string;
  invite_scope: RoomAppearance["inviteScope"];
  participant_type: "human";
  client_type: "browser";
  provider_kind: string;
  connection_kind: string;
  expires_at: string;
  room_label: string;
  room_topic: string;
  room_created_at: string;
};

export type RoomInviteJoinResponse = AdmissionSessionBase & {
  status: "admitted";
  request_id: string;
  avatar_image_url?: string;
  owner_display_name: string;
  owner_id: string;
  stable_identity: boolean;
  operator: boolean;
  client_id: string;
  guide: {
    welcome: string;
    how_to: string[];
    etiquette: string[];
    session: { expires_in_seconds: number; rejoin: string };
  };
};

export type OperatorPairingRedeemResponse = AdmissionSessionBase & {
  status: "admitted";
  owner_id: string;
  stable_identity: true;
  operator: true;
};

export type GuestRecoveryRedeemResponse = AdmissionSessionBase & {
  status: "recovered";
  client_id: string;
  room_uid: string;
  joined_at: string;
  recovery_code: string;
};

type InvitePreflightContext = {
  room_id: string;
  room_label: string;
  invite_scope: RoomAppearance["inviteScope"];
};

export type RoomInviteAdmissionResponse =
  | ({ status: "profile_required"; can_auto_join: false } & InvitePreflightContext)
  | ({
      status: "existing_session" | "existing_member" | "known_user";
      can_auto_join: true;
      participant: {
        participant_id: string;
        display_name: string;
        avatar_image_url: string;
      };
      operator: boolean;
    } & InvitePreflightContext)
  | ({
      status: "agent_client_required";
      reason: "agent_client_required";
      can_auto_join: false;
    } & InvitePreflightContext)
  | {
      status: "invite_invalid" | "invite_expired";
      reason: string;
      can_auto_join: false;
    };

const SURFACE_KEYS = [
  "server_id",
  "authority_lineage_id",
  "server_product_surface",
] as const;

const SESSION_KEYS = [
  "session_token",
  "agent_id",
  "display_name",
  "meeting_id",
  "invite_scope",
  "participant_type",
  "client_type",
  "provider_kind",
  "connection_kind",
  "expires_at",
  "room_label",
  "room_topic",
  "room_created_at",
] as const;

function validateTimestamp(value: string, label: string) {
  if (Number.isNaN(Date.parse(value))) {
    throw new Error(`${label}가 올바른 시간이 아닙니다.`);
  }
}

function validateInviteScope(
  payload: Record<string, unknown>,
  label: string
): RoomAppearance["inviteScope"] {
  const inviteScope = requiredString(payload, "invite_scope", label);
  if (inviteScope !== "room" && inviteScope !== "read_only") {
    throw new Error(`${label}.invite_scope가 올바르지 않습니다.`);
  }
  return inviteScope;
}

function parseInvitePreflightContext(
  payload: Record<string, unknown>,
  label: string
): InvitePreflightContext {
  return {
    room_id: requiredString(payload, "room_id", label),
    room_label: requiredString(payload, "room_label", label),
    invite_scope: validateInviteScope(payload, label),
  };
}

function validateCommon(
  payload: Record<string, unknown>,
  label: string
): AdmissionSessionBase {
  const surface = parseRoomSessionSurface({
    server_id: payload.server_id,
    authority_lineage_id: payload.authority_lineage_id,
    server_product_surface: payload.server_product_surface,
  });
  const inviteScope = validateInviteScope(payload, label);
  if (payload.participant_type !== "human" || payload.client_type !== "browser") {
    throw new Error(`${label}의 참가자 또는 클라이언트 유형이 올바르지 않습니다.`);
  }
  const expiresAt = requiredString(payload, "expires_at", label);
  const roomCreatedAt = requiredString(payload, "room_created_at", label);
  validateTimestamp(expiresAt, `${label}.expires_at`);
  validateTimestamp(roomCreatedAt, `${label}.room_created_at`);
  return {
    ...surface,
    session_token: requiredString(payload, "session_token", label),
    agent_id: requiredString(payload, "agent_id", label),
    display_name: requiredString(payload, "display_name", label),
    meeting_id: requiredString(payload, "meeting_id", label),
    invite_scope: inviteScope,
    participant_type: "human",
    client_type: "browser",
    provider_kind: requiredString(payload, "provider_kind", label),
    connection_kind: requiredString(payload, "connection_kind", label),
    expires_at: expiresAt,
    room_label: requiredString(payload, "room_label", label),
    room_topic: stringField(payload, "room_topic", label),
    room_created_at: roomCreatedAt,
  };
}

export function parseRoomInviteAdmissionResponse(
  value: unknown
): RoomInviteAdmissionResponse {
  const label = "방 입장 사전 확인";
  const payload = strictRecord(value, label);
  if (payload.status === "invite_invalid" || payload.status === "invite_expired") {
    assertExactKeys(payload, ["status", "reason", "can_auto_join"], label);
    if (payload.can_auto_join !== false) {
      throw new Error("거절된 방 입장 사전 확인 상태가 올바르지 않습니다.");
    }
    return {
      status: payload.status,
      reason: requiredString(payload, "reason", label),
      can_auto_join: false,
    };
  }
  if (payload.status === "agent_client_required") {
    assertExactKeys(
      payload,
      [
        "status",
        "reason",
        "can_auto_join",
        "room_id",
        "room_label",
        "invite_scope",
      ],
      label
    );
    if (payload.reason !== "agent_client_required" || payload.can_auto_join !== false) {
      throw new Error("에이전트 전용 방 입장 사전 확인 상태가 올바르지 않습니다.");
    }
    return {
      status: "agent_client_required",
      reason: "agent_client_required",
      can_auto_join: false,
      ...parseInvitePreflightContext(payload, label),
    };
  }
  const status = payload.status;
  const recognized =
    status === "existing_session" ||
    status === "existing_member" ||
    status === "known_user";
  const expectedKeys = recognized
    ? [
        "status",
        "can_auto_join",
        "room_id",
        "room_label",
        "invite_scope",
        "participant",
        "operator",
      ]
    : ["status", "can_auto_join", "room_id", "room_label", "invite_scope"];
  assertExactKeys(payload, expectedKeys, label);
  if (status !== "profile_required" && !recognized) {
    throw new Error("방 입장 사전 확인 상태가 올바르지 않습니다.");
  }
  const context = parseInvitePreflightContext(payload, label);
  if (!recognized) {
    if (payload.can_auto_join !== false) {
      throw new Error("프로필 입력 사전 확인 상태가 올바르지 않습니다.");
    }
    return { status: "profile_required", can_auto_join: false, ...context };
  }
  const participant = strictRecord(payload.participant, `${label}.participant`);
  assertExactKeys(
    participant,
    ["participant_id", "display_name", "avatar_image_url"],
    `${label}.participant`
  );
  if (payload.can_auto_join !== true || typeof payload.operator !== "boolean") {
    throw new Error("자동 방 입장 사전 확인 상태가 올바르지 않습니다.");
  }
  return {
    status,
    can_auto_join: true,
    ...context,
    participant: {
      participant_id: requiredString(participant, "participant_id", label),
      display_name: requiredString(participant, "display_name", label),
      avatar_image_url: stringField(participant, "avatar_image_url", label),
    },
    operator: payload.operator,
  };
}

function validateGuide(value: unknown): RoomInviteJoinResponse["guide"] {
  const guide = strictRecord(value, "방 입장 안내");
  assertExactKeys(guide, ["welcome", "how_to", "etiquette", "session"], "방 입장 안내");
  const session = strictRecord(guide.session, "방 입장 안내.session");
  assertExactKeys(session, ["expires_in_seconds", "rejoin"], "방 입장 안내.session");
  if (
    !Array.isArray(guide.how_to) ||
    guide.how_to.some((entry) => typeof entry !== "string") ||
    !Array.isArray(guide.etiquette) ||
    guide.etiquette.some((entry) => typeof entry !== "string") ||
    !Number.isSafeInteger(session.expires_in_seconds) ||
    Number(session.expires_in_seconds) <= 0
  ) {
    throw new Error("방 입장 안내 응답이 올바르지 않습니다.");
  }
  return {
    welcome: requiredString(guide, "welcome", "방 입장 안내"),
    how_to: [...guide.how_to],
    etiquette: [...guide.etiquette],
    session: {
      expires_in_seconds: Number(session.expires_in_seconds),
      rejoin: requiredString(session, "rejoin", "방 입장 안내.session"),
    },
  };
}

export function parseRoomInviteJoinResponse(
  value: unknown,
  expectedRequestId: string,
  expectedRoomId: string,
  expectedClientId: string
): RoomInviteJoinResponse {
  const label = "방 입장";
  const payload = strictRecord(value, label);
  assertExactKeys(
    payload,
    [
      "status",
      "request_id",
      ...SESSION_KEYS,
      "owner_display_name",
      "owner_id",
      "stable_identity",
      "operator",
      "client_id",
      "guide",
      ...SURFACE_KEYS,
    ],
    label,
    ["avatar_image_url"]
  );
  if (payload.status !== "admitted") {
    throw new Error("방 입장 상태가 올바르지 않습니다.");
  }
  if (requiredString(payload, "request_id", label) !== expectedRequestId) {
    throw new Error("방 입장 응답이 현재 요청과 일치하지 않습니다.");
  }
  const common = validateCommon(payload, label);
  if (!expectedRoomId || common.meeting_id !== expectedRoomId) {
    throw new Error("방 입장 응답이 확인된 방과 일치하지 않습니다.");
  }
  const clientId = requiredString(payload, "client_id", label);
  if (!expectedClientId || clientId !== expectedClientId) {
    throw new Error("방 입장 응답이 현재 클라이언트와 일치하지 않습니다.");
  }
  if (typeof payload.stable_identity !== "boolean" || typeof payload.operator !== "boolean") {
    throw new Error("방 입장 신원 상태가 올바르지 않습니다.");
  }
  return {
    ...common,
    status: "admitted",
    request_id: expectedRequestId,
    avatar_image_url: optionalString(payload, "avatar_image_url", label),
    owner_display_name: stringField(payload, "owner_display_name", label),
    owner_id: requiredString(payload, "owner_id", label),
    stable_identity: payload.stable_identity,
    operator: payload.operator,
    client_id: clientId,
    guide: validateGuide(payload.guide),
  };
}

export function parseOperatorPairingRedeemResponse(
  value: unknown
): OperatorPairingRedeemResponse {
  const label = "운영자 연결";
  const payload = strictRecord(value, label);
  assertExactKeys(
    payload,
    [
      "status",
      ...SESSION_KEYS,
      "owner_id",
      "stable_identity",
      "operator",
      ...SURFACE_KEYS,
    ],
    label
  );
  if (payload.status !== "admitted" || payload.stable_identity !== true || payload.operator !== true) {
    throw new Error("운영자 연결 신원 상태가 올바르지 않습니다.");
  }
  return {
    ...validateCommon(payload, label),
    status: "admitted",
    owner_id: requiredString(payload, "owner_id", label),
    stable_identity: true,
    operator: true,
  };
}

export function parseGuestRecoveryRedeemResponse(
  value: unknown,
  expectedRoomId: string,
  expectedClientId: string
): GuestRecoveryRedeemResponse {
  const label = "게스트 신원 복구";
  const payload = strictRecord(value, label);
  assertExactKeys(
    payload,
    [
      "status",
      ...SESSION_KEYS,
      "client_id",
      "room_uid",
      "joined_at",
      "recovery_code",
      ...SURFACE_KEYS,
    ],
    label
  );
  if (payload.status !== "recovered") {
    throw new Error("게스트 신원 복구 상태가 올바르지 않습니다.");
  }
  const common = validateCommon(payload, label);
  if (common.meeting_id !== expectedRoomId) {
    throw new Error("복구된 신원이 요청한 방과 일치하지 않습니다.");
  }
  const clientId = requiredString(payload, "client_id", label);
  if (clientId !== expectedClientId) {
    throw new Error("복구된 신원이 현재 클라이언트와 일치하지 않습니다.");
  }
  const joinedAt = requiredString(payload, "joined_at", label);
  validateTimestamp(joinedAt, `${label}.joined_at`);
  return {
    ...common,
    status: "recovered",
    client_id: clientId,
    room_uid: requiredString(payload, "room_uid", label),
    joined_at: joinedAt,
    recovery_code: requiredString(payload, "recovery_code", label),
  };
}
