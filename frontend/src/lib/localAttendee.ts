import { localizeBootstrapError } from "./apiErrors";
import { parseAttendeeEntryPacket } from "../api/attendeeInvite";
import { strictRecord, requiredString } from "./strictJsonContract";
import type { AttendeeEntryPacket } from "../types/generated/AttendeeEntryPacket";
import type { LocalAttendeePhase } from "../types/generated/LocalAttendeePhase";

export const LOCAL_ATTENDEE_PHASE_LABELS: Record<LocalAttendeePhase, string> = {
  admitting: "방에 참가하고 있어요.",
  admission_unresolved: "참가 결과를 확인하지 못했어요. 같은 요청으로 다시 확인해 주세요.",
  admitted: "방에 추가했어요. 이 컴퓨터에서 실행할 준비가 됐어요.",
  starting: "이 컴퓨터에서 에이전트를 시작하고 있어요.",
  running: "이 컴퓨터에서 실행 중이에요.",
  stopping: "에이전트를 종료하고 방에서 나가는 중이에요.",
  stopped: "에이전트 종료와 방 나가기를 확인했어요.",
  failed: "에이전트 실행이 중단됐어요. 방에서 에이전트 추가를 다시 해 주세요.",
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
  if (!id || !encoded || encoded.length > 8192 || [...fields.keys()].length !== 1) throw new Error("방에서 에이전트 추가를 다시 해 주세요.");
  const packet = strictRecord(JSON.parse(encoded), "AI 참가 초대");
  return parseAttendeeEntryPacket(packet, {
    requestId: id, roomId: requiredString(packet, "room_id", "AI 참가 초대"),
    roomUid: requiredString(packet, "room_uid", "AI 참가 초대"),
  }).result;
}

export function localAttendeeFailureReason(code: string): string {
  switch (code) {
    case "room_observation_unconfirmed": return "AI가 전달받은 방 내용을 읽었다고 확인하지 못했어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "room_portal_publication_missing": return "AI가 방에 답변이나 응답 보류를 제출하지 않았어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "local_attendee_process_restarted": return "앱이 다시 시작되어 이전 실행 결과를 확인할 수 없어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "invalid_workspace": case "workspace_authority_changed": return "작업 폴더를 확인할 수 없어요. 사용할 폴더를 다시 선택해 주세요.";
    case "executable_authority_changed": return "AI 실행 파일이 변경되었거나 사용할 수 없어요. AI 목록을 새로고침한 뒤 다시 추가해 주세요.";
    case "provider_protocol_timeout": return "AI 프로그램의 응답 시간이 초과됐어요. AI 로그인 상태를 확인하고 다시 추가해 주세요.";
    case "provider_runtime_exited": case "provider_process_exited": return "AI 프로그램이 종료됐어요. AI 설치와 로그인 상태를 확인해 주세요.";
    case "attendee_execution_authority_mismatch": case "runtime_owner_mismatch": return "전달받은 작업이 이 컴퓨터의 AI 실행과 일치하지 않아요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "runtime_profile_changed": return "AI 실행 설정이 변경되어 작업을 계속할 수 없어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "session_revoked": case "permission_denied": case "operator_pairing_unavailable": return "방 접속 또는 AI 참가 권한이 해제됐어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "attendee_session_expired": case "invite_unavailable": return "AI 참가 초대나 접속이 만료됐어요. 방에서 에이전트 추가를 다시 해 주세요.";
    case "attendee_cleanup_unresolved": case "attendee_runtime_cleanup_unconfirmed": return "AI 종료 또는 방 나가기를 확인하지 못했어요. 상태를 다시 확인해 주세요.";
    default: return "AI 작업을 완료하지 못했어요. 앱의 실행 로그에서 실패 원인을 확인해 주세요.";
  }
}

export function localAttendeeErrorMessage(failure: unknown, fallback: string): string {
  const message = failure instanceof Error ? failure.message : typeof failure === "string" ? failure : "";
  return localizeBootstrapError(message.trim()) || fallback;
}
