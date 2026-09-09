import type { ReleaseHealthStatus } from "../types/generated/ReleaseHealthStatus";

const LABELS: Record<ReleaseHealthStatus, string> = {
  passed: "통과", failed: "실패", unavailable: "실행 도구 없음", timed_out: "시간 초과",
  cancelled: "취소", cleanup_unconfirmed: "프로세스 종료 확인 필요", not_run: "미실행",
};
export function releaseHealthStatusLabel(status: ReleaseHealthStatus) {
  return LABELS[status];
}
