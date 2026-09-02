import type { RoomAgentSession, RoomEvent, RoomMember } from "../api";
import type { AgentSession } from "../types/generated/AgentSession";
import type { Participant } from "../types/generated/Participant";
import type { PersonaAssetSummary } from "../types/generated/PersonaAssetSummary";
import { isParticipantRole } from "./participantRole";
import {
  assertExactKeys,
  strictRecord,
  type ExactGeneratedKeys,
} from "./strictJsonContract";

const GENERATED_PARTICIPANT_KEYS = [
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
] as const satisfies readonly (keyof Participant)[];
const PARTICIPANT_KEYS: ExactGeneratedKeys<
  Participant,
  typeof GENERATED_PARTICIPANT_KEYS
> = GENERATED_PARTICIPANT_KEYS;

const GENERATED_AGENT_SESSION_KEYS = [
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
  "persona_card_id",
  "persona_card",
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
] as const satisfies readonly (keyof AgentSession)[];

const AGENT_SESSION_KEYS: ExactGeneratedKeys<
  AgentSession,
  typeof GENERATED_AGENT_SESSION_KEYS
> = GENERATED_AGENT_SESSION_KEYS;

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

const AGENT_SESSION_STRING_KEYS = AGENT_SESSION_KEYS.filter(
  (key) =>
    key !== "persona_card" &&
    !AGENT_SESSION_BOOLEAN_KEYS.includes(
      key as (typeof AGENT_SESSION_BOOLEAN_KEYS)[number]
    ) &&
    !AGENT_SESSION_INTEGER_KEYS.includes(
      key as (typeof AGENT_SESSION_INTEGER_KEYS)[number]
    )
);

const PERSONA_SUMMARY_KEYS = [
  "id",
  "display_name",
  "asset_kind",
  "source_kind",
  "lorebook_count",
  "asset_count",
  "ignored_feature_count",
  "tag_count",
  "thumbnail_url",
] as const satisfies readonly (keyof PersonaAssetSummary)[];
const GENERATED_PERSONA_SUMMARY_KEYS: ExactGeneratedKeys<
  PersonaAssetSummary,
  typeof PERSONA_SUMMARY_KEYS
> = PERSONA_SUMMARY_KEYS;

function personaSummaryMatches(value: unknown, personaCardId: string): boolean {
  if (value === null) return personaCardId === "";
  if (!personaCardId) return false;
  let persona: Record<string, unknown>;
  try {
    persona = exactEventRecord(
      value,
      GENERATED_PERSONA_SUMMARY_KEYS,
      "Agent Session 페르소나 투영이 없습니다.",
      "Agent Session 페르소나 투영이 올바르지 않습니다."
    );
  } catch {
    return false;
  }
  const integerKeys = [
    "lorebook_count",
    "asset_count",
    "ignored_feature_count",
    "tag_count",
  ] as const;
  return (
    persona.id === personaCardId &&
    ["id", "display_name", "source_kind", "thumbnail_url"].every(
      (key) => typeof persona[key] === "string"
    ) &&
    (persona.asset_kind === "card" || persona.asset_kind === "module") &&
    integerKeys.every(
      (key) => Number.isSafeInteger(persona[key]) && Number(persona[key]) >= 0
    )
  );
}

function exactEventRecord(
  value: unknown,
  keys: readonly string[],
  missingMessage: string,
  invalidMessage: string
): Record<string, unknown> {
  let record: Record<string, unknown>;
  try {
    record = strictRecord(value, missingMessage);
  } catch {
    throw new Error(missingMessage);
  }
  try {
    assertExactKeys(record, keys, invalidMessage);
  } catch {
    throw new Error(invalidMessage);
  }
  return record;
}

function exactAgentSession(
  value: unknown,
  missingMessage: string,
  invalidMessage: string,
): Record<string, unknown> {
  const session = exactEventRecord(
    value,
    AGENT_SESSION_KEYS,
    missingMessage,
    invalidMessage,
  );
  if (
    AGENT_SESSION_STRING_KEYS.some((key) => typeof session[key] !== "string") ||
    AGENT_SESSION_BOOLEAN_KEYS.some((key) => typeof session[key] !== "boolean") ||
    AGENT_SESSION_INTEGER_KEYS.some(
      (key) => !Number.isSafeInteger(session[key]) || Number(session[key]) < 0
    ) ||
    !session.room_id ||
    !session.session_id ||
    !session.participant_id ||
    !personaSummaryMatches(session.persona_card, session.persona_card_id as string)
  ) {
    throw new Error(invalidMessage);
  }
  return session;
}

function exactParticipant(
  value: unknown,
  missingMessage: string,
  invalidMessage: string,
): Record<string, unknown> {
  const participant = exactEventRecord(
    value,
    PARTICIPANT_KEYS,
    missingMessage,
    invalidMessage,
  );
  if (
    PARTICIPANT_KEYS.filter((key) => key !== "muted").some(
      (key) => typeof participant[key] !== "string"
    ) ||
    typeof participant.muted !== "boolean" ||
    !isParticipantRole(participant.role) ||
    !participant.room_id ||
    !participant.participant_id
  ) {
    throw new Error(invalidMessage);
  }
  return participant;
}

export function agentSessionProjectionsMatch(left: unknown, right: unknown): boolean {
  try {
    const leftSession = exactAgentSession(left, "Agent Session", "Agent Session");
    const rightSession = exactAgentSession(right, "Agent Session", "Agent Session");
    return AGENT_SESSION_KEYS.every((key) => {
      if (key !== "persona_card") return leftSession[key] === rightSession[key];
      if (leftSession[key] === null || rightSession[key] === null) {
        return leftSession[key] === rightSession[key];
      }
      const leftPersona = strictRecord(leftSession[key], "Agent Session persona");
      const rightPersona = strictRecord(rightSession[key], "Agent Session persona");
      return GENERATED_PERSONA_SUMMARY_KEYS.every(
        (personaKey) => leftPersona[personaKey] === rightPersona[personaKey]
      );
    });
  } catch {
    return false;
  }
}

export function participantProjectionsMatch(left: unknown, right: unknown): boolean {
  try {
    const leftParticipant = exactParticipant(left, "Participant", "Participant");
    const rightParticipant = exactParticipant(right, "Participant", "Participant");
    return PARTICIPANT_KEYS.every(
      (key) => leftParticipant[key] === rightParticipant[key]
    );
  } catch {
    return false;
  }
}

export function agentSessionIsValid(
  value: unknown,
  expectedRoomId = "",
  expectedParticipantId = "",
): value is RoomAgentSession {
  try {
    const session = exactAgentSession(
      value,
      "Agent Session 투영이 없습니다.",
      "Agent Session 투영이 올바르지 않습니다.",
    );
    return (
      (!expectedRoomId || session.room_id === expectedRoomId) &&
      (!expectedParticipantId || session.participant_id === expectedParticipantId)
    );
  } catch {
    return false;
  }
}

function participantFromEvent(
  event: RoomEvent,
  expectedStatus: "joined" | "detached"
): RoomMember {
  const participant = exactParticipant(
    (event as unknown as Record<string, unknown>).participant,
    `${event.type} 이벤트에 참가자 투영이 없습니다.`,
    `${event.type} 이벤트의 참가자 투영이 올바르지 않습니다.`
  );
  if (
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
  const session = exactAgentSession(
    eventRecord.agent_session,
    "agent_session_created 이벤트에 Agent Session 투영이 없습니다.",
    "agent_session_created 이벤트의 Agent Session 투영이 올바르지 않습니다."
  );
  if (
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
