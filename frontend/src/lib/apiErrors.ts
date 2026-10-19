import { CENTRAL_THROTTLED_NOTICE } from "./central/centralRequestGovernor";

export const MEMBER_SESSION_ENDED_MESSAGE =
  "이 방의 접속이 해제됐어요. 다시 참가하려면 방장에게 문의해 주세요.";

export const GUEST_SESSION_EXPIRED_MESSAGE =
  "방 접속이 만료되거나 해제됐어요. 호스트에서 새 접속 링크를 받아 주세요.";

export class ApiError extends Error {
  status: number;
  code: string;
  resolution?: "rejected" | "unresolved";

  constructor(status: number, message: string, code = "", resolution?: "rejected" | "unresolved") {
    super(localizeBootstrapError(message));
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.resolution = resolution;
  }
}

export function isUnauthorizedApiError(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}

// Single owner for user-facing error text. Rendered notices must go through
// this function: known product/server codes map to their Korean text,
// product-authored Korean messages pass through, and everything else
// (transport failures, AbortError/cancellation, SyntaxError, English server
// text, unknown) falls back to the caller's neutral Korean line.
const KNOWN_ERROR_KOREAN: Record<string, string> = {
  server_move_unsupported: "서버 옮기기는 아직 지원하지 않아요.",
  duplicate_resolution_conflict: "다른 기기에서 서버를 선택했어요. 목록을 다시 확인해 주세요.",
  server_retirement_capacity: "잠시 후 다시 시도해 주세요.",
  actor_quota_exhausted: "오늘 요청 한도를 다 썼어요. 잠시 후 다시 시도해 주세요.",
  temporary_capacity_exhausted: "오늘 요청 한도를 다 썼어요. 잠시 후 다시 시도해 주세요.",
  central_throttled: CENTRAL_THROTTLED_NOTICE,
  room_attachment_quota_reached: "이 방의 저장 공간이 가득 찼어요.",
  attachment_quota_reached: "이 방의 저장 공간이 가득 찼어요.",
  attachment_type_unsupported: "올릴 수 없는 파일 형식이에요.",
  provider_login_unsupported: "이 AI는 앱에서 로그인을 지원하지 않아요.",
  provider_login_missing: "AI CLI를 설치한 뒤 로그인해 주세요.",
  provider_login_timeout: "로그인 시간이 초과됐어요. 다시 시도해 주세요.",
  provider_login_cancelled: "로그인을 취소했어요.",
  provider_login_failed: "로그인하지 못했어요. 로그인 창을 확인한 뒤 다시 시도해 주세요.",
  provider_login_handoff_unconfirmed: "터미널이 열렸을 수 있어요. 확인한 뒤 카탈로그를 갱신해 주세요.",
  provider_login_cleanup_unconfirmed: "로그인 프로세스가 종료됐는지 확인하지 못했어요.",
  workspace_picker_timeout: "폴더 선택 창이 응답하지 않아요. 창을 닫고 다시 시도해 주세요.",
  workspace_picker_unavailable: "이 기기에서는 폴더 선택 창을 열 수 없어요.",
  workspace_picker_unsupported_platform: "이 기기에서는 폴더 선택 창을 열 수 없어요.",
  workspace_picker_invalid_selection: "선택한 폴더를 사용할 수 없어요. 다른 폴더를 선택해 주세요.",
  workspace_picker_failed: "폴더 선택 창을 열지 못했어요. 앱을 앞으로 가져온 뒤 다시 시도해 주세요.",
};

function errorCodeOf(error: unknown): string {
  if (error !== null && typeof error === "object") {
    const record = error as { code?: unknown; category?: unknown };
    if (typeof record.code === "string" && record.code.trim()) return record.code.trim();
    if (typeof record.category === "string" && record.category.trim()) return record.category.trim();
  }
  return "";
}

// Internal ids, timestamps, and transport jargon never reach the screen,
// even inside an otherwise-Korean line (e.g. entry/parser diagnostics).
function looksLikeInternalId(line: string): boolean {
  return (
    /\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b/.test(line) ||
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(line) ||
    /server_id|registration_epoch|invite_token|challenge_id|grant_token/i.test(line)
  );
}

function firstKoreanLine(value: string): string {
  const line = String(value).split("\n").map((part) => part.trim()).find((part) => part.length > 0) ?? "";
  return line;
}

// Product copy is written in 해요체; formal '~습니다' lines are internal contract
// and invariant checks (authority, surface, ACK wording) the caller's own
// sentence should replace.
function isInternalCheck(line: string): boolean {
  return /습니다[.!]?$/.test(line.trim());
}

export function userFacingError(error: unknown, fallback: string): string {
  const safeFallback = firstKoreanLine(fallback) || "다시 시도해 주세요.";
  const code = errorCodeOf(error);
  if (code && KNOWN_ERROR_KOREAN[code]) return KNOWN_ERROR_KOREAN[code];
  // Substring codes (workspace picker) arrive inside the message.
  if (error instanceof Error && typeof error.message === "string") {
    for (const [key, text] of Object.entries(KNOWN_ERROR_KOREAN)) {
      if (key.startsWith("workspace_picker_") && error.message.includes(key)) return text;
    }
  }
  // Transport/cancellation/parser failures never surface raw text.
  if (error instanceof TypeError) return safeFallback;
  if (error instanceof SyntaxError) return safeFallback;
  if (typeof DOMException !== "undefined" && error instanceof DOMException) return safeFallback;
  if (error instanceof Error) {
    const line = firstKoreanLine(error.message);
    if (line && /[가-힣]/.test(line) && !looksLikeInternalId(line) && !isInternalCheck(line)) return line;
    return safeFallback;
  }
  if (typeof error === "string") {
    const line = firstKoreanLine(error);
    if (line && /[가-힣]/.test(line) && !looksLikeInternalId(line) && !isInternalCheck(line)) return line;
    return safeFallback;
  }
  return safeFallback;
}

export function localizeBootstrapError(message: string): string {
  return message === "Local identity bootstrap is not complete."
    ? "이 컴퓨터의 사용자 설정이 완료되지 않았어요. 앱에서 로그인한 뒤 상태를 다시 확인해 주세요."
    : message;
}
