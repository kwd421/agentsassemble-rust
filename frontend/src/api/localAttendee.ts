import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { agentCreationPayload, type FrontendLiveAgentCreateRequest } from "./agentSessions";
import { LOCAL_ATTENDEE_PHASE_LABELS } from "../lib/localAttendee";
import { assertExactKeys, strictRecord } from "../lib/strictJsonContract";
import type { AttendeeEntryPacket } from "../types/generated/AttendeeEntryPacket";
import type { LocalAttendeeStatus } from "../types/generated/LocalAttendeeStatus";
import type { LocalAttendeeAction } from "../types/generated/LocalAttendeeAction";
import type { LocalAttendeeCreate } from "../types/generated/LocalAttendeeCreate";

export function localAttendeeCreateRequest(packet: AttendeeEntryPacket, request: FrontendLiveAgentCreateRequest): LocalAttendeeCreate {
  if (request.sessionId || request.providerId !== packet.provider || request.meetingId !== packet.room_id) throw new Error("초대와 선택한 제공자·방이 일치하지 않아요.");
  return { request_id: packet.request_id, room_id: packet.room_id, room_uid: packet.room_uid,
    invite_url: packet.join_url, creation: agentCreationPayload(request) };
}

export async function createLocalAttendee(packet: AttendeeEntryPacket, request: LocalAttendeeCreate): Promise<LocalAttendeeStatus> {
  return parseStatus(await postJsonServerOperator<unknown>("/api/local-attendees", request), packet);
}

export async function fetchLocalAttendee(packet: AttendeeEntryPacket, signal?: AbortSignal): Promise<LocalAttendeeStatus> {
  return parseStatus(await fetchJsonServerOperator<unknown>(`/api/local-attendees/${encodeURIComponent(packet.request_id)}`, undefined, signal), packet);
}

export async function commandLocalAttendee(packet: AttendeeEntryPacket, action: LocalAttendeeAction): Promise<LocalAttendeeStatus> {
  return parseStatus(await postJsonServerOperator<unknown>(`/api/local-attendees/${encodeURIComponent(packet.request_id)}`, { action }), packet);
}

function parseStatus(input: unknown, packet: AttendeeEntryPacket): LocalAttendeeStatus {
  const value = strictRecord(input, "내 PC 참가 상태");
  assertExactKeys(value, ["request_id", "room_id", "room_uid", "participant_id", "phase", "error_code"], "내 PC 참가 상태");
  if (value.request_id !== packet.request_id || value.room_id !== packet.room_id || value.room_uid !== packet.room_uid ||
      typeof value.phase !== "string" || !Object.hasOwn(LOCAL_ATTENDEE_PHASE_LABELS, value.phase) ||
      (value.participant_id !== null && (typeof value.participant_id !== "string" || !value.participant_id)) ||
      (value.error_code !== null && typeof value.error_code !== "string")) throw new Error("참가 상태가 현재 요청과 일치하지 않아요.");
  return value as LocalAttendeeStatus;
}
