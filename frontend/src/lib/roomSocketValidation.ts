import type { RoomEvent } from "../api";
import { RoomSocketSayError } from "../roomSocketTypes";
import { ROOM_HISTORY_MAX_EVENTS } from "../types/generated/ROOM_HISTORY_WIRE";
import {
  agentCreationProjectionFromEvent,
  joinedParticipantFromEvent,
} from "./participantEventContract";
import { isParticipantRole } from "./participantRole";
import { voteSummaryResultIsValid } from "./roomVoteSummaryContract";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function isSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function participantProjectionIsValid(event: RoomEvent): boolean {
  try {
    if (event.type === "participant_joined") joinedParticipantFromEvent(event);
    if (event.type === "agent_session_created") agentCreationProjectionFromEvent(event);
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
  return Boolean(
    isRecord(value) &&
    typeof value.id === "string" &&
    value.id &&
    value.room_id === expectedRoomId &&
    typeof value.type === "string" &&
    value.type &&
    isSequence(value.seq) &&
    value.seq > 0 &&
    participantProjectionIsValid(value as unknown as RoomEvent)
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
    typeof event.id === "string" &&
    event.id &&
    event.room_id === expectedRoomId &&
    isSequence(event.seq) &&
    event.seq > 0 &&
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
  if (action === "participant.kick") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      participant.participant_id === payload.participant_id &&
      participant.status === "kicked"
    );
  }
  if (action === "participant.role.update") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      participant &&
      event &&
      participant.participant_id === payload.participant_id &&
      participant.role === payload.role &&
      event.type === "participant_updated" &&
      event.participant_id === payload.participant_id &&
      event.role === payload.role
    );
  }
  if (action === "room.settings.update") {
    return Boolean(isRecord(result.room_settings) && event?.type === "room_settings_updated");
  }
  if (action === "participant.mute") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      hasDurableEvent &&
      participant &&
      event?.type === "participant_muted" &&
      participant.participant_id === payload.participant_id &&
      participant.muted === Boolean(payload.muted) &&
      event.participant_id === payload.participant_id &&
      event.muted === payload.muted
    );
  }
  if (action === "participant.leave") {
    const participant = isRecord(result.participant) ? result.participant : null;
    return Boolean(
      hasDurableEvent &&
      participant &&
      participant.room_id === expectedRoomId &&
      participant.participant_id === expectedParticipantId &&
      participant.status === "left" &&
      event?.type === "participant_left" &&
      event.participant_id === expectedParticipantId
    );
  }
  if (action === "room.delete") return result.deleted === true;
  if (action === "provider.request.resolve") {
    return Boolean(
      result.status === "resolving" &&
      result.provider_request_id === payload.provider_request_id &&
      event?.type === "provider_request_resolution_requested"
    );
  }
  if (action === "agent.create" || action === "agent.configure") {
    return isRecord(result.agent_session);
  }
  if (action === "agent.readd") return result.status === "readded";
  if (action.startsWith("agent.")) return isRecord(result.agent_session);
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
    !isRecord(value.room_settings) ||
    !Array.isArray(value.participants) ||
    !Array.isArray(value.agent_sessions) ||
    !Array.isArray(value.active_turns) ||
    !Array.isArray(value.events) ||
    !isRecord(value.provider_catalog) ||
    !Array.isArray(value.available_providers) ||
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
