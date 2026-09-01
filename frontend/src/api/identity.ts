import {
  postJson,
  postJsonWithIdentity,
} from "./http";
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

export function issueGuestRecoveryCode({
  sessionToken,
  deviceToken,
}: {
  sessionToken: string;
  deviceToken?: string;
}): Promise<GuestRecoveryCodeResponse> {
  return postJsonWithIdentity<GuestRecoveryCodeResponse>(
    "/api/identity/recovery-code",
    {},
    { sessionToken, deviceToken }
  );
}

export function redeemGuestRecoveryCode({
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
  return postJson<unknown>("/api/identity/recovery-code/redeem", {
    recovery_code: recoveryCode,
    room_id: roomId,
    device_token: deviceToken,
    client_id: clientId,
  }).then((payload) => parseGuestRecoveryRedeemResponse(payload, roomId, clientId));
}
