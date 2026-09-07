import {
  parseDesktopManagerRoomAuthority,
  type DesktopManagerRoomAuthority,
} from "../lib/desktopBridge";
import { parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import { assertExactKeys, requiredString, strictRecord } from "../lib/strictJsonContract";
import { postJsonServerOperator } from "./http";

export type OperatorPairingCustody = Readonly<{
  authority: DesktopManagerRoomAuthority;
  pairingId: string;
  pairingUrl: string;
  origin: string;
  expiresAt: string;
  expiresAtMs: number;
}>;

export async function createOperatorPairing(
  authority: DesktopManagerRoomAuthority,
  beforeDispatch: () => void
): Promise<OperatorPairingCustody> {
  const exactAuthority = parseDesktopManagerRoomAuthority(authority);
  const response = strictRecord(await postJsonServerOperator<unknown>(
    "/api/operator-pairing/create", exactAuthority, beforeDispatch
  ), "기기 연결");
  assertExactKeys(response, ["pairing_id", "pairing_url", "expires_at"], "기기 연결");
  const pairingId = requiredString(response, "pairing_id", "기기 연결");
  const pairingUrl = requiredString(response, "pairing_url", "기기 연결");
  const expiresAt = requiredString(response, "expires_at", "기기 연결");
  const expiresAtMs = Date.parse(expiresAt);
  let url: URL;
  try {
    url = new URL(pairingUrl);
    parsePublicIngressOrigin(url.origin);
  } catch {
    throw new Error("기기 연결 주소를 확인할 수 없어요.");
  }
  if (
    !Number.isFinite(expiresAtMs) ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(pairingId) ||
    pairingUrl !== url.toString() || url.pathname !== "/pair" ||
    url.username || url.password || url.hash ||
    !/^\?token=aap1\.[A-Za-z0-9_-]{43}$/.test(url.search)
  ) {
    throw new Error("기기 연결 응답을 확인할 수 없어요.");
  }
  return Object.freeze({
    authority: exactAuthority, pairingId, pairingUrl,
    origin: url.origin, expiresAt, expiresAtMs,
  });
}

export async function revokeOperatorPairing(
  custody: OperatorPairingCustody,
  beforeDispatch: () => void
) {
  const response = strictRecord(await postJsonServerOperator<unknown>(
    "/api/operator-pairing/revoke",
    { authority: custody.authority, pairing_id: custody.pairingId },
    beforeDispatch
  ), "기기 연결 해제");
  assertExactKeys(response, ["status", "pairing_id"], "기기 연결 해제");
  if (response.status !== "revoked" || response.pairing_id !== custody.pairingId) {
    throw new Error("연결 해제 결과를 확인할 수 없어요. 다시 시도해 주세요.");
  }
}
