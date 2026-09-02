import type { RoomEvent } from "../api";
import { RoomSocketSayError } from "../roomSocketTypes";
import type { Actor } from "../types/generated/Actor";
import type { PublicRoomSettings } from "../types/generated/PublicRoomSettings";
import type { RoomAppearance } from "../types/generated/RoomAppearance";
import type { RoomChannel } from "../types/generated/RoomChannel";
import { ROOM_HISTORY_MAX_EVENTS } from "../types/generated/ROOM_HISTORY_WIRE";
import {
  agentCreateAckProjectionsAreCoherent,
  agentCreationProjectionFromEvent,
  agentSessionIsValid,
  agentSessionStateProjectionIsCoherent,
  joinedParticipantFromEvent,
  participantIsValid,
} from "./participantEventContract";
import { isParticipantRole } from "./participantRole";
import { providerCatalogIsValid } from "./providerCatalogContract";
import { voteSummaryResultIsValid } from "./roomVoteSummaryContract";
import {
  assertExactKeys,
  strictRecord,
  type ExactGeneratedKeys,
} from "./strictJsonContract";

const GENERATED_ACTOR_KEYS = ["participant_id", "participant_type"] as const;
const ACTOR_KEYS: ExactGeneratedKeys<Actor, typeof GENERATED_ACTOR_KEYS> =
  GENERATED_ACTOR_KEYS;
const GENERATED_ROOM_SETTINGS_KEYS = [
  "settings_revision",
  "label",
  "topic",
  "appearance",
  "conversation_mode",
  "tool_mode",
  "ordered_exclude_previous_speaker",
  "channels",
  "activity_plugin",
] as const;
const ROOM_SETTINGS_KEYS: ExactGeneratedKeys<
  PublicRoomSettings,
  typeof GENERATED_ROOM_SETTINGS_KEYS
> = GENERATED_ROOM_SETTINGS_KEYS;
const GENERATED_APPEARANCE_KEYS = [
  "banner_preset",
  "banner_image_url",
  "icon_image_url",
  "icon_label",
  "invite_scope",
] as const;
const APPEARANCE_KEYS: ExactGeneratedKeys<
  RoomAppearance,
  typeof GENERATED_APPEARANCE_KEYS
> = GENERATED_APPEARANCE_KEYS;
const GENERATED_CHANNEL_KEYS = [
  "id",
  "name",
  "type",
  "position",
  "created_at",
] as const;
const CHANNEL_KEYS: ExactGeneratedKeys<RoomChannel, typeof GENERATED_CHANNEL_KEYS> =
  GENERATED_CHANNEL_KEYS;
const ROOM_EVENT_OPTIONAL_STRING_KEYS = [
  "participant_id",
  "participant_type",
  "actor_id",
  "actor_type",
  "display_name",
  "content",
  "message_kind",
] as const satisfies readonly (keyof RoomEvent)[];

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function isSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function publicRoomSettingsIsValid(value: unknown): value is PublicRoomSettings {
  try {
    const settings = strictRecord(value, "room settings");
    assertExactKeys(settings, ROOM_SETTINGS_KEYS, "room settings");
    const appearance = strictRecord(settings.appearance, "room settings appearance");
    assertExactKeys(appearance, APPEARANCE_KEYS, "room settings appearance");
    if (
      ["settings_revision", "label", "topic", "conversation_mode", "tool_mode", "activity_plugin"]
        .some((key) => typeof settings[key] !== "string") ||
      typeof settings.ordered_exclude_previous_speaker !== "boolean" ||
      APPEARANCE_KEYS.some((key) => typeof appearance[key] !== "string") ||
      !Array.isArray(settings.channels)
    ) {
      return false;
    }
    return settings.channels.every((value) => {
      const channel = strictRecord(value, "room settings channel");
      assertExactKeys(channel, CHANNEL_KEYS, "room settings channel");
      return (
        ["id", "name", "type", "created_at"].every(
          (key) => typeof channel[key] === "string"
        ) &&
        (channel.type === "text" || channel.type === "voice") &&
        Number.isSafeInteger(channel.position) &&
        Number(channel.position) >= 0
      );
    });
  } catch {
    return false;
  }
}

function publicRoomSettingsMatch(left: unknown, right: unknown): boolean {
  if (!publicRoomSettingsIsValid(left) || !publicRoomSettingsIsValid(right)) {
    return false;
  }
  return ROOM_SETTINGS_KEYS.every((key) => {
    if (key === "appearance") {
      return APPEARANCE_KEYS.every(
        (appearanceKey) => left.appearance[appearanceKey] === right.appearance[appearanceKey]
      );
    }
    if (key === "channels") {
      return (
        left.channels.length === right.channels.length &&
        left.channels.every((channel, index) =>
          CHANNEL_KEYS.every(
            (channelKey) => channel[channelKey] === right.channels[index][channelKey]
          )
        )
      );
    }
    return left[key] === right[key];
  });
}

export function eventProjectionIsValid(event: RoomEvent): boolean {
  try {
    if (
      event.type === "room_settings_updated" &&
      !publicRoomSettingsIsValid(
        (event as unknown as Record<string, unknown>).room_settings
      )
    ) {
      return false;
    }
    if (event.type === "participant_joined") joinedParticipantFromEvent(event);
    if (event.type === "agent_session_created") agentCreationProjectionFromEvent(event);
    if (
      event.type === "agent_session_state" &&
      !agentSessionStateProjectionIsCoherent(event)
    ) return false;
    if (
      event.type === "participant_updated" &&
      "role" in event &&
      !isParticipantRole(event.role)
    ) {
      return false;
    }
    if (event.type === "participant_muted") {
      if (
        typeof event.participant_id !== "string" ||
        !event.participant_id ||
        typeof event.muted !== "boolean"
      ) {
        return false;
      }
    }
    return true;
  } catch {
    return false;
  }
}

export function publicRoomEventIsValid(value: unknown, expectedRoomId: string): value is RoomEvent {
  const actor = isRecord(value) && isRecord(value.actor) ? value.actor : null;
  return Boolean(
    isRecord(value) &&
    value.v === 1 &&
    typeof value.id === "string" &&
    value.id &&
    typeof value.created_at === "string" &&
    value.created_at &&
    value.room_id === expectedRoomId &&
    typeof value.type === "string" &&
    value.type &&
    actor &&
    Object.keys(actor).length === ACTOR_KEYS.length &&
    ACTOR_KEYS.every(
      (key) => typeof actor[key] === "string" && Boolean(actor[key])
    ) &&
    ROOM_EVENT_OPTIONAL_STRING_KEYS.every(
      (key) =>
        value[key] === undefined ||
        value[key] === null ||
        typeof value[key] === "string"
    ) &&
    isSequence(value.seq) &&
    value.seq > 0 &&
    eventProjectionIsValid(value as unknown as RoomEvent)
  );
}

function roomHistoryResultIsValid(
  payload: Record<string, unknown>,
  result: Record<string, unknown>,
  expectedRoomId: string
): boolean {
  const beforeSeq = payload.before_seq;
  const limit = payload.limit;
  if (
    !isSequence(beforeSeq) ||
    typeof limit !== "number" ||
    !Number.isSafeInteger(limit) ||
    limit < 1 ||
    limit > ROOM_HISTORY_MAX_EVENTS ||
    !Array.isArray(result.events) ||
    result.events.length > limit ||
    !isSequence(result.oldest_seq) ||
    !isSequence(result.last_seq) ||
    typeof result.has_more_before !== "boolean"
  ) {
    return false;
  }
  const sequences: number[] = [];
  for (const event of result.events) {
    if (
      !publicRoomEventIsValid(event, expectedRoomId) ||
      (beforeSeq > 0 && event.seq >= beforeSeq)
    ) {
      return false;
    }
    sequences.push(event.seq);
  }
  if (
    sequences.some(
      (sequence, index) => index > 0 && sequence !== sequences[index - 1] + 1
    )
  ) {
    return false;
  }
  if (!sequences.length) {
    return (
      result.oldest_seq === 0 &&
      result.has_more_before === false &&
      (result.last_seq === 0 || beforeSeq === 1)
    );
  }
  const firstSeq = sequences[0];
  const finalSeq = sequences[sequences.length - 1];
  const expectedFinalSeq =
    beforeSeq === 0
      ? result.last_seq
      : Math.min(result.last_seq, beforeSeq - 1);
  return Boolean(
    result.oldest_seq === firstSeq &&
    finalSeq === expectedFinalSeq &&
    result.has_more_before === (firstSeq > 1)
  );
}

export function commandAckResultIsValid(
  action: string,
  payload: Record<string, unknown>,
  result: unknown,
  expectedRoomId: string,
  expectedParticipantId: string
): boolean {
  if (!isRecord(result)) return false;
  const event = isRecord(result.event) ? result.event : null;
  const hasDurableEvent = Boolean(
    event &&
    publicRoomEventIsValid(event, expectedRoomId) &&
    result.event_seq === event.seq
  );
  if (action === "message.send" || action.startsWith("room.random.")) {
    return hasDurableEvent && event?.type === "message_final";
  }
  if (action === "message.edit" || action === "message.delete") {
    return Boolean(
      hasDurableEvent &&
      event?.type === (action === "message.edit" ? "message_updated" : "message_deleted") &&
      event?.target_event_id === payload.event_id
    );
  }
  if (action === "room.history") {
    return roomHistoryResultIsValid(payload, result, expectedRoomId);
  }
  if (action === "participant.role.update") {
    const participant = participantIsValid(result.participant, expectedRoomId)
      ? result.participant
      : null;
    return Boolean(
      hasDurableEvent &&
      participant &&
      event &&
      participant.participant_id === payload.participant_id &&
      participant.role === payload.role &&
      event.type === "participant_updated" &&
      event.participant_id === payload.participant_id &&
      event.participant_type === participant.participant_type &&
      event.display_name === participant.display_name &&
      event.role === payload.role
    );
  }
  if (action === "room.settings.update") {
    return Boolean(
      hasDurableEvent &&
      event?.type === "room_settings_updated" &&
      publicRoomSettingsMatch(result.room_settings, event.room_settings)
    );
  }
  if (action === "participant.mute") {
    const participant = participantIsValid(result.participant, expectedRoomId)
      ? result.participant
      : null;
    return Boolean(
      hasDurableEvent &&
      participant &&
      event?.type === "participant_muted" &&
      participant.participant_id === payload.participant_id &&
      participant.muted === Boolean(payload.muted) &&
      event.participant_id === payload.participant_id &&
      event.participant_type === participant.participant_type &&
      event.display_name === participant.display_name &&
      event.muted === payload.muted
    );
  }
  if (action === "participant.leave") {
    const participant = participantIsValid(result.participant, expectedRoomId)
      ? result.participant
      : null;
    return Boolean(
      hasDurableEvent &&
      participant &&
      participant.room_id === expectedRoomId &&
      participant.participant_id === expectedParticipantId &&
      participant.status === "left" &&
      event?.type === "participant_left" &&
      event.participant_id === expectedParticipantId &&
      event.participant_type === participant.participant_type &&
      event.display_name === participant.display_name
    );
  }
  if (action.startsWith("agent.")) {
    if (action === "agent.create") {
      if (
        !hasDurableEvent ||
        !Array.isArray(result.events) ||
        !result.events.every((candidate) =>
          publicRoomEventIsValid(candidate, expectedRoomId)
        )
      ) {
        return false;
      }
      if (isRecord(result.start)) {
        if (
          !Array.isArray(result.start.events) ||
          !result.start.events.every((candidate) =>
            publicRoomEventIsValid(candidate, expectedRoomId)
          ) ||
          !publicRoomEventIsValid(result.start.event, expectedRoomId)
        ) {
          return false;
        }
      }
      return agentCreateAckProjectionsAreCoherent(payload, result);
    }
    const expectedAgentId = String(payload.agent_id || "");
    return agentSessionIsValid(result.agent_session, expectedRoomId, expectedAgentId);
  }
  if (action === "room.vote.summary") {
    return voteSummaryResultIsValid(payload, result);
  }
  return true;
}

export function snapshotValidationError(
  value: unknown,
  {
    expectedRoomId,
    currentLastSeq,
  }: { expectedRoomId: string; currentLastSeq: number }
): RoomSocketSayError | null {
  if (
    !isRecord(value) ||
    value.op !== "snapshot" ||
    value.stream !== "room_events" ||
    !isRecord(value.room) ||
    value.room.room_id !== expectedRoomId ||
    !publicRoomSettingsIsValid(value.room_settings) ||
    !Array.isArray(value.participants) ||
    !Array.isArray(value.agent_sessions) ||
    !Array.isArray(value.active_turns) ||
    !Array.isArray(value.events) ||
    !providerCatalogIsValid(value.provider_catalog) ||
    !Array.isArray(value.available_providers) ||
    JSON.stringify(value.available_providers) !==
      JSON.stringify(value.provider_catalog.providers) ||
    !isRecord(value.capabilities) ||
    typeof value.has_more_before !== "boolean" ||
    typeof value.resume_gap !== "boolean"
  ) {
    return new RoomSocketSayError(
      "Room snapshot did not match the canonical browser schema; reconnecting.",
      "snapshot_schema_invalid"
    );
  }
  const mode = value.snapshot_mode;
  if (mode !== "initial" && mode !== "resume" && mode !== "gap") {
    return new RoomSocketSayError(
      "Room snapshot used an invalid browser snapshot mode; reconnecting.",
      "snapshot_mode_invalid"
    );
  }
  if (
    value.resume_gap !== (mode === "gap") ||
    (mode === "initial" && currentLastSeq !== 0) ||
    (mode !== "initial" && currentLastSeq <= 0) ||
    !isSequence(value.oldest_seq) ||
    !isSequence(value.last_seq) ||
    value.last_seq < currentLastSeq
  ) {
    return new RoomSocketSayError(
      "Room snapshot sequence metadata was inconsistent; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  const sequences: number[] = [];
  const participantIds = new Set<string>();
  const agentParticipantIds = new Set<string>();
  for (const participant of value.participants) {
    if (
      !participantIsValid(participant, expectedRoomId) ||
      participantIds.has(participant.participant_id)
    ) {
      return new RoomSocketSayError(
        "Room snapshot contained an invalid Participant projection; reconnecting.",
        "snapshot_participant_invalid"
      );
    }
    participantIds.add(participant.participant_id);
    if (participant.participant_type === "agent") {
      agentParticipantIds.add(participant.participant_id);
    }
  }
  const sessionIds = new Set<string>();
  const sessionParticipantIds = new Set<string>();
  for (const session of value.agent_sessions) {
    if (
      !agentSessionIsValid(session, expectedRoomId) ||
      sessionIds.has(session.session_id) ||
      sessionParticipantIds.has(session.participant_id) ||
      !agentParticipantIds.has(session.participant_id)
    ) {
      return new RoomSocketSayError(
        "Room snapshot contained an invalid Agent Session projection; reconnecting.",
        "snapshot_agent_session_invalid"
      );
    }
    sessionIds.add(session.session_id);
    sessionParticipantIds.add(session.participant_id);
  }
  if (
    [...agentParticipantIds].some(
      (participantId) => !sessionParticipantIds.has(participantId)
    )
  ) {
    return new RoomSocketSayError(
      "Room snapshot contained an Agent participant without its session; reconnecting.",
      "snapshot_agent_session_invalid"
    );
  }
  for (const event of value.events) {
    if (
      !publicRoomEventIsValid(event, expectedRoomId)
    ) {
      return new RoomSocketSayError(
        "Room snapshot contained an invalid canonical event; reconnecting.",
        "snapshot_event_invalid"
      );
    }
    sequences.push(event.seq);
  }
  if (sequences.some((sequence, index) => index > 0 && sequence !== sequences[index - 1] + 1)) {
    return new RoomSocketSayError(
      "Room snapshot event sequence was not contiguous; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  if (!sequences.length) {
    const validEmptyBoundary =
      value.oldest_seq === 0 &&
      ((mode === "initial" && value.last_seq === 0) ||
        (mode === "resume" && value.last_seq === currentLastSeq));
    return validEmptyBoundary
      ? null
      : new RoomSocketSayError(
          "Room snapshot omitted events required by its sequence boundary; reconnecting.",
          "snapshot_sequence_invalid"
        );
  }
  const firstSeq = sequences[0];
  const finalSeq = sequences[sequences.length - 1];
  if (
    value.oldest_seq !== firstSeq ||
    value.last_seq !== finalSeq ||
    (mode === "resume" && firstSeq !== currentLastSeq + 1)
  ) {
    return new RoomSocketSayError(
      "Room snapshot event range did not match its durable cursor; reconnecting.",
      "snapshot_sequence_invalid"
    );
  }
  return null;
}
