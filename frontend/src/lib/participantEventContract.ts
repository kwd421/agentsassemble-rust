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
const PARTICIPANT_TYPES = ["human", "agent"] as const;
const PARTICIPANT_STATUSES = ["joined", "left", "kicked", "exported", "detached"] as const;

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
    !PARTICIPANT_TYPES.includes(
      participant.participant_type as (typeof PARTICIPANT_TYPES)[number]
    ) ||
    !PARTICIPANT_STATUSES.includes(
      participant.status as (typeof PARTICIPANT_STATUSES)[number]
    ) ||
    !participant.room_id ||
    !participant.participant_id
  ) {
    throw new Error(invalidMessage);
  }
  return participant;
}

export function participantIsValid(
  value: unknown,
  expectedRoomId = "",
): value is Participant {
  try {
    const participant = exactParticipant(value, "Participant", "Participant");
    return !expectedRoomId || participant.room_id === expectedRoomId;
  } catch {
    return false;
  }
}

function personaProjectionsMatch(left: unknown, right: unknown): boolean {
  if (left === null || right === null) return left === right;
  try {
    const leftPersona = strictRecord(left, "Agent Session persona");
    const rightPersona = strictRecord(right, "Agent Session persona");
    return GENERATED_PERSONA_SUMMARY_KEYS.every(
      (key) => leftPersona[key] === rightPersona[key]
    );
  } catch {
    return false;
  }
}

export function agentSessionProjectionsMatch(left: unknown, right: unknown): boolean {
  try {
    const leftSession = exactAgentSession(left, "Agent Session", "Agent Session");
    const rightSession = exactAgentSession(right, "Agent Session", "Agent Session");
    return AGENT_SESSION_KEYS.every((key) => {
      if (key !== "persona_card") return leftSession[key] === rightSession[key];
      return personaProjectionsMatch(leftSession[key], rightSession[key]);
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

function eventProjectionsMatch(left: unknown, right: unknown): boolean {
  if (!left || !right || typeof left !== "object" || typeof right !== "object") {
    return false;
  }
  return JSON.stringify(left) === JSON.stringify(right);
}

function creationStartSessionsMatch(initial: unknown, prepared: unknown): boolean {
  try {
    const initialSession = exactAgentSession(initial, "Agent Session", "Agent Session");
    const preparedSession = exactAgentSession(prepared, "Agent Session", "Agent Session");
    const transitionKeys = new Set(["runtime_status", "enabled", "updated_at"]);
    return (
      initialSession.status === "available" &&
      initialSession.runtime_status === "stopped" &&
      initialSession.enabled === false &&
      preparedSession.status === "available" &&
      preparedSession.runtime_status === "starting" &&
      preparedSession.enabled === true &&
      AGENT_SESSION_KEYS.every((key) => {
        if (transitionKeys.has(key)) return true;
        if (key !== "persona_card") return initialSession[key] === preparedSession[key];
        return personaProjectionsMatch(initialSession[key], preparedSession[key]);
      })
    );
  } catch {
    return false;
  }
}

function creationStartedSessionMatches(prepared: unknown, final: unknown): boolean {
  try {
    const preparedSession = exactAgentSession(prepared, "Agent Session", "Agent Session");
    const finalSession = exactAgentSession(final, "Agent Session", "Agent Session");
    const transitionKeys = new Set([
      "status",
      "runtime_status",
      "provider_session_active",
      "provider_session_reused",
      "updated_at",
    ]);
    return (
      finalSession.status === "attached" &&
      finalSession.runtime_status === "idle" &&
      finalSession.enabled === true &&
      finalSession.provider_session_active === true &&
      AGENT_SESSION_KEYS.every((key) => {
        if (transitionKeys.has(key)) return true;
        if (key !== "persona_card") return preparedSession[key] === finalSession[key];
        return personaProjectionsMatch(preparedSession[key], finalSession[key]);
      })
    );
  } catch {
    return false;
  }
}

function creationParticipantBecomesJoined(created: RoomMember, joined: RoomMember): boolean {
  return (
    created.status === "detached" &&
    joined.status === "joined" &&
    [
      "meeting_id",
      "participant_id",
      "display_name",
      "avatar_image_url",
      "participant_type",
      "role",
      "owner_id",
      "muted",
      "created_at",
    ].every(
      (key) =>
        created[key as keyof RoomMember] === joined[key as keyof RoomMember]
    )
  );
}

export function agentCreateAckProjectionsAreCoherent(
  payload: Record<string, unknown>,
  result: Record<string, unknown>,
): boolean {
  try {
    if (result.status !== "created" || !Array.isArray(result.events)) return false;
    const events = result.events as RoomEvent[];
    const createdEvent = events[0];
    if (
      !createdEvent ||
      createdEvent.type !== "agent_session_created" ||
      events.some((event, index) => index > 0 && event.seq !== events[index - 1].seq + 1)
    ) {
      return false;
    }
    const created = agentCreationProjectionFromEvent(createdEvent);
    if (
      !participantProjectionsMatch(result.participant, createdEvent.participant) ||
      !eventProjectionsMatch(result.event, events.at(-1))
    ) {
      return false;
    }

    const startRequested = payload.start === true || payload.start_now === true;
    if (!startRequested) {
      return (
        events.length === 1 &&
        !("start" in result) &&
        agentSessionProjectionsMatch(result.agent_session, createdEvent.agent_session) &&
        agentSessionProjectionsMatch(
          (result.event as Record<string, unknown>).agent_session,
          createdEvent.agent_session,
        ) &&
        participantProjectionsMatch(
          (result.event as Record<string, unknown>).participant,
          createdEvent.participant,
        )
      );
    }

    if (
      events.length !== 4 ||
      events[1].type !== "participant_joined" ||
      events[2].type !== "session_attached" ||
      events[3].type !== "agent_session_state" ||
      !creationStartSessionsMatch(result.agent_session, createdEvent.agent_session)
    ) {
      return false;
    }
    const joined = joinedParticipantFromEvent(events[1]);
    const finalSession = (events[3] as unknown as Record<string, unknown>).agent_session;
    const start = exactEventRecord(
      result.start,
      ["agent_session", "runtime_reused", "events", "event"],
      "agent.create 시작 투영이 없습니다.",
      "agent.create 시작 투영이 올바르지 않습니다.",
    );
    if (
      !creationParticipantBecomesJoined(created.participant, joined) ||
      events[2].participant_id !== created.participant.participant_id ||
      events[3].participant_id !== created.participant.participant_id ||
      typeof start.runtime_reused !== "boolean" ||
      !Array.isArray(start.events) ||
      start.events.length !== 3 ||
      !start.events.every((event, index) => eventProjectionsMatch(event, events[index + 1])) ||
      !eventProjectionsMatch(start.event, events[3]) ||
      !participantProjectionsMatch(
        (start.events[0] as Record<string, unknown>).participant,
        (events[1] as unknown as Record<string, unknown>).participant,
      ) ||
      !agentSessionProjectionsMatch(
        (start.events[2] as Record<string, unknown>).agent_session,
        finalSession,
      ) ||
      !agentSessionProjectionsMatch(start.agent_session, finalSession) ||
      !agentSessionProjectionsMatch(
        (start.event as Record<string, unknown>).agent_session,
        finalSession,
      ) ||
      !agentSessionProjectionsMatch(
        (result.event as Record<string, unknown>).agent_session,
        finalSession,
      )
    ) {
      return false;
    }
    return creationStartedSessionMatches(created.agentSession, finalSession);
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

export function agentSessionStateProjectionIsCoherent(event: RoomEvent): boolean {
  try {
    const eventRecord = event as unknown as Record<string, unknown>;
    const session = exactAgentSession(
      eventRecord.agent_session,
      "Agent Session 투영이 없습니다.",
      "Agent Session 투영이 올바르지 않습니다.",
    );
    return (
      eventRecord.participant_type === "agent" &&
      session.room_id === event.room_id &&
      session.participant_id === eventRecord.participant_id &&
      session.session_id === eventRecord.session_id &&
      session.runtime_status === eventRecord.runtime_status &&
      session.display_name === eventRecord.display_name
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
