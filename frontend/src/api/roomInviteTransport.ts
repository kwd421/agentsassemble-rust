import { sessionHostOrigin } from "../lib/remote/remoteWorkspace";
import { fetchProductTransport } from "../lib/remote/remoteWorkspace";
import { assertExactKeys, requiredString, strictRecord } from "../lib/strictJsonContract";
import { parsePublicIngressOrigin } from "../lib/publicIngressStatus";

/** An admitted central-owner room session; never a native operator credential. */
export type RemoteInviteTransport = { sessionToken: string; deviceToken: string };

export function fetchRemoteRoomInvite(
  path: string, authority: RemoteInviteTransport, init: RequestInit,
  beforeDispatch?: () => void,
): Promise<Response> {
  if (!authority.sessionToken || !authority.deviceToken) {
    throw new Error("현재 방의 소유자 접속을 확인할 수 없어요.");
  }
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${authority.sessionToken}`);
  headers.set("X-Device-Token", authority.deviceToken);
  beforeDispatch?.();
  return fetchProductTransport(`/api/central-owner${path}`, {
    ...init, headers, cache: "no-store", redirect: "error", credentials: "omit",
  });
}

export async function fetchRemoteInviteOrigin(authority: RemoteInviteTransport, beforeDispatch: () => void) {
  const response = await fetchRemoteRoomInvite("/public-invite/origin", authority, { method: "GET" }, beforeDispatch);
  if (!response.ok) throw new Error("현재 서버의 초대 주소를 확인할 수 없어요. 접속 권한을 확인해 주세요.");
  const value = strictRecord(await response.json(), "초대 주소");
  assertExactKeys(value, ["public_url"], "초대 주소");
  const public_url = parsePublicIngressOrigin(requiredString(value, "public_url", "초대 주소"));
  if (public_url !== sessionHostOrigin(authority.sessionToken)) throw new Error("서버 주소가 변경됐어요. 서버 목록에서 다시 연결해 주세요.");
  return { public_url, remote: true as const };
}
