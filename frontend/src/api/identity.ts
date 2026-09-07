import { responseError } from "./http";
import { assertExactKeys, requiredString, strictRecord } from "../lib/strictJsonContract";
import {
  parseGuestRecoveryRedeemResponse,
  type GuestRecoveryRedeemResponse,
} from "../lib/roomAdmissionContract";

export type GuestRecoveryCodeResponse = {
  status: "issued";
  server_id: string;
  room_id: string;
  recovery_code: string;
  recovery_url: string;
};

export type { GuestRecoveryRedeemResponse };

export async function issueGuestRecoveryCode({
  sessionToken,
  deviceToken,
}: {
  sessionToken: string;
  deviceToken?: string;
}): Promise<GuestRecoveryCodeResponse> {
  if (!sessionToken || !deviceToken) throw new Error("현재 방 세션과 기기 정보가 필요해요.");
  const response = await fetch("/api/identity/recovery-code", {
    method: "POST", cache: "no-store", redirect: "error",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${sessionToken}`, "X-Device-Token": deviceToken },
    body: "{}",
  });
  if (!response.ok) throw await responseError(response);
  const payload = strictRecord(await response.json(), "복구 코드 발급");
  assertExactKeys(payload, ["status", "server_id", "room_id", "recovery_code", "recovery_url"], "복구 코드 발급");
  if (payload.status !== "issued") throw new Error("복구 코드 발급 결과를 확인하지 못했어요.");
  return {
    status: "issued",
    server_id: requiredString(payload, "server_id", "복구 코드 발급"),
    room_id: requiredString(payload, "room_id", "복구 코드 발급"),
    recovery_code: requiredString(payload, "recovery_code", "복구 코드 발급"),
    recovery_url: requiredString(payload, "recovery_url", "복구 코드 발급"),
  };
}

export async function redeemGuestRecoveryCode({
  recoveryCode,
  roomId,
  deviceToken,
  clientId,
}: {
  recoveryCode: string;
  roomId: string;
  deviceToken: string;
  clientId: string;
}): Promise<GuestRecoveryRedeemResponse> {
  const response = await fetch("/api/identity/recovery-code/redeem", {
    method: "POST", cache: "no-store", redirect: "error",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ recovery_code: recoveryCode, room_id: roomId, device_token: deviceToken, client_id: clientId }),
  });
  if (!response.ok) throw await responseError(response);
  return parseGuestRecoveryRedeemResponse(await response.json(), roomId, clientId);
}
