import type { RoomAgentSession, RoomEvent, RoomMember } from "../api";

const PARTICIPANT_KEYS = [
  "room_id",
  "participant_id",
  "display_name",
  "avatar_image_url",
  "participant_type",
  "status",
  "role",
  "owner_id",
  "muted",
  "created_at",
  "updated_at",
] as const;

const AGENT_SESSION_KEYS = [
  "room_id",
  "session_id",
  "participant_id",
  "display_name",
  "status",
  "runtime_status",
  "enabled",
  "provider_kind",
  "runtime_kind",
  "connection_kind",
  "external_owned",
  "process_ownership",
  "model",
  "reasoning_effort",
  "service_tier",
  "variant",
  "execution_harness",
  "permission_mode",
  "max_output_tokens",
  "catalog_revision",
  "transport",
  "last_seen_event_id",
  "last_seen_seq",
  "last_provider_sync_event_id",
  "last_provider_sync_seq",
  "bootstrap_cutoff_seq",
  "turn_count",
  "active_turn_id",
  "turn_phase",
  "last_error",
  "last_error_code",
  "recovery_required",
  "provider_session_active",
  "provider_session_reused",
  "created_at",
  "updated_at",
] as const;

const AGENT_SESSION_BOOLEAN_KEYS = [
  "enabled",
  "external_owned",
  "recovery_required",
  "provider_session_active",
  "provider_session_reused",
] as const;

const AGENT_SESSION_INTEGER_KEYS = [
  "max_output_tokens",
  "last_seen_seq",
  "last_provider_sync_seq",
  "bootstrap_cutoff_seq",
  "turn_count",
] as const;

function strictRecord(
  value: unknown,
  keys: readonly string[],
  missingMessage: string,
  invalidMessage: string
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(missingMessage);
  }
  const record = value as Record<string, unknown>;
  const actualKeys = Object.keys(record).sort();
  const expectedKeys = [...keys].sort();
  if (
    actualKeys.length !== expectedKeys.length ||
    actualKeys.some((key, index) => key !== expectedKeys[index])
  ) {
    throw new Error(invalidMessage);
  }
  return record;
}

function participantFromEvent(
  event: RoomEvent,
  expectedStatus: "joined" | "detached"
): RoomMember {
  const participant = strictRecord(
    (event as unknown as Record<string, unknown>).participant,
    PARTICIPANT_KEYS,
    `${event.type} 이벤트에 참가자 투영이 없습니다.`,
    `${event.type} 이벤트의 참가자 투영이 올바르지 않습니다.`
  );
  if (
    PARTICIPANT_KEYS.filter((key) => key !== "muted").some(
      (key) => typeof participant[key] !== "string"
    ) ||
    typeof participant.muted !== "boolean" ||
    !participant.participant_id ||
    participant.participant_id !== event.participant_id ||
    participant.room_id !== event.room_id ||
    participant.status !== expectedStatus
  ) {
    throw new Error(`${event.type} 이벤트의 참가자 투영이 올바르지 않습니다.`);
  }
  return {
    meeting_id: participant.room_id as string,
    participant_id: participant.participant_id as string,
    display_name: participant.display_name as string,
    avatar_image_url: participant.avatar_image_url as string,
    role: participant.role as RoomMember["role"],
    participant_type: participant.participant_type as RoomMember["participant_type"],
    provider_kind: "",
    connection_kind: "",
    owner_id: participant.owner_id as string,
    status: participant.status as string,
    muted: participant.muted as boolean,
    source: participant.participant_type === "human" ? "room" : "agent_session",
    created_at: participant.created_at as string,
    updated_at: participant.updated_at as string,
  };
}

export function joinedParticipantFromEvent(event: RoomEvent): RoomMember {
  return participantFromEvent(event, "joined");
}

export function agentCreationProjectionFromEvent(event: RoomEvent): {
  participant: RoomMember;
  agentSession: RoomAgentSession;
} {
  const eventRecord = event as unknown as Record<string, unknown>;
  const participant = participantFromEvent(event, "detached");
  const session = strictRecord(
    eventRecord.agent_session,
    AGENT_SESSION_KEYS,
    "agent_session_created 이벤트에 Agent Session 투영이 없습니다.",
    "agent_session_created 이벤트의 Agent Session 투영이 올바르지 않습니다."
  );
  const stringKeys = AGENT_SESSION_KEYS.filter(
    (key) =>
      !AGENT_SESSION_BOOLEAN_KEYS.includes(key as (typeof AGENT_SESSION_BOOLEAN_KEYS)[number]) &&
      !AGENT_SESSION_INTEGER_KEYS.includes(key as (typeof AGENT_SESSION_INTEGER_KEYS)[number])
  );
  if (
    stringKeys.some((key) => typeof session[key] !== "string") ||
    AGENT_SESSION_BOOLEAN_KEYS.some((key) => typeof session[key] !== "boolean") ||
    AGENT_SESSION_INTEGER_KEYS.some(
      (key) => !Number.isSafeInteger(session[key]) || Number(session[key]) < 0
    ) ||
    !session.session_id ||
    session.room_id !== event.room_id ||
    session.session_id !== eventRecord.session_id ||
    session.session_id !== session.participant_id ||
    session.participant_id !== event.participant_id ||
    session.participant_id !== participant.participant_id ||
    session.provider_kind !== eventRecord.provider_kind ||
    session.display_name !== participant.display_name ||
    session.display_name !== event.display_name ||
    event.participant_type !== "agent" ||
    String(participant.participant_type) !== "agent" ||
    participant.role !== "agent" ||
    session.status !== "available" ||
    !(
      (session.runtime_status === "stopped" && session.enabled === false) ||
      (session.runtime_status === "starting" && session.enabled === true)
    ) ||
    session.external_owned !== false ||
    session.process_ownership !== "server"
  ) {
    throw new Error("agent_session_created 이벤트의 생성 투영이 올바르지 않습니다.");
  }
  return {
    participant,
    agentSession: session as unknown as RoomAgentSession,
  };
}
