import { parseAttendeeEntryPacket } from "../api/attendeeInvite";
import { strictRecord, requiredString } from "./strictJsonContract";
import type { AttendeeEntryPacket } from "../types/generated/AttendeeEntryPacket";
import type { LocalAttendeePhase } from "../types/generated/LocalAttendeePhase";

export const LOCAL_ATTENDEE_PHASE_LABELS: Record<LocalAttendeePhase, string> = {
  admitting: "방에 참가하고 있어요.",
  admission_unresolved: "참가 결과를 확인하지 못했어요. 같은 요청으로 다시 확인해 주세요.",
  admitted: "방에 추가했어요. 이 PC에서 실행할 준비가 됐어요.",
  starting: "이 PC에서 에이전트를 시작하고 있어요.",
  running: "이 PC에서 실행 중이에요.",
  stopping: "에이전트를 종료하고 방에서 나가는 중이에요.",
  stopped: "에이전트 종료와 방 나가기를 확인했어요.",
  failed: "에이전트를 시작하지 못했어요. 브라우저에서 새 초대를 만들어 다시 시도해 주세요.",
  cleanup_unconfirmed: "작업 정리를 확인하지 못했어요. 새 실행을 시작하지 않고 현재 결과를 유지해요.",
};

export function localAttendeeLink(packet: AttendeeEntryPacket): string {
  const fragment = new URLSearchParams({ packet: JSON.stringify(packet) });
  return `agentsassemble://attend#${fragment}`;
}

export function readLocalAttendeeHandoff(url: URL): AttendeeEntryPacket {
  const id = url.searchParams.get("attendee-create");
  const fields = new URLSearchParams(url.hash.slice(1));
  const encoded = fields.get("packet");
  if (!id || !encoded || encoded.length > 8192 || [...fields.keys()].length !== 1) throw new Error("브라우저에서 내 PC의 에이전트 추가 화면을 다시 열어 주세요.");
  const packet = strictRecord(JSON.parse(encoded), "AI 참가 초대");
  return parseAttendeeEntryPacket(packet, {
    requestId: id, roomId: requiredString(packet, "room_id", "AI 참가 초대"),
    roomUid: requiredString(packet, "room_uid", "AI 참가 초대"),
  }).result;
}
