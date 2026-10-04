import type { CentralOwnerSessionStatus } from "../types/generated/CentralOwnerSessionStatus";
import { assertExactKeys, strictRecord } from "./strictJsonContract";

export function parseOwnerSessionStatus(value: unknown): CentralOwnerSessionStatus {
  const record = strictRecord(value, "서버 세션 상태");
  if (record.state === "active") {
    assertExactKeys(record, ["state"], "서버 세션 상태");
    return { state: "active" };
  }
  assertExactKeys(record, ["state", "reason"], "서버 세션 상태");
  if (record.state !== "ended" || !["revoked", "disconnected", "unavailable"].includes(String(record.reason))) {
    throw new Error("서버 세션 상태를 확인하지 못했어요.");
  }
  return { state: "ended", reason: record.reason as "revoked" | "disconnected" | "unavailable" };
}
