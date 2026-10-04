import type { CentralOwnerSessionStatus } from "../types/generated/CentralOwnerSessionStatus";
import { assertExactKeys, strictRecord } from "./strictJsonContract";

export function parseOwnerSessionStatus(value: unknown): CentralOwnerSessionStatus {
  const record = strictRecord(value, "서버 세션 상태");
  if (record.state === "active" || record.state === "retrying") {
    assertExactKeys(record, ["state", "expires_at"], "서버 세션 상태");
    if (!Number.isSafeInteger(record.expires_at) || Number(record.expires_at) <= Date.now() / 1000 ||
        Number(record.expires_at) > Date.now() / 1000 + 60) throw new Error("서버 세션 유효시간을 확인하지 못했어요.");
    return { state: record.state, expires_at: Number(record.expires_at) };
  }
  assertExactKeys(record, ["state", "reason"], "서버 세션 상태");
  if (record.state !== "ended" || !["revoked", "expired", "unavailable"].includes(String(record.reason))) {
    throw new Error("서버 세션 상태를 확인하지 못했어요.");
  }
  return { state: "ended", reason: record.reason as "revoked" | "expired" | "unavailable" };
}
