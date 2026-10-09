import { isDesktopWebview, requestDesktopAccountDeletionTicket } from "../desktopBridge";
import { loadCentralSession, signedRequest, type CentralSession, type CentralServer } from "./identity";
import { strictRecord, assertExactKeys } from "../strictJsonContract";

export type OwnDeletionHost = { server_id: string; registration_epoch: string; fingerprint: string; key: string };
/** Signed current endpoint tuple is the only release floor; registration/cache alone is insufficient. */
export async function deletionOwnHost(session: CentralSession, signal: AbortSignal): Promise<{ host: OwnDeletionHost | null; hasOwnServers: boolean }> {
  const live = () => { signal.throwIfAborted(); if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요."); };
  live();
  const root = strictRecord(await signedRequest<unknown>(session, "/v1/bootstrap", "GET", undefined, signal), "탈퇴 지원 확인");
  live();
  if (strictRecord(root.person,"계정").person_id !== session.person.person_id || !Array.isArray(root.servers) || root.servers.length > 512) throw new Error("내 서버의 탈퇴 지원을 확인하지 못했어요.");
  for (const row of root.servers) { const server = strictRecord(row, "내 서버"); if (typeof server.server_id !== "string" || !server.server_id || !["owner", "bookmark", "member"].includes(String(server.relation))) throw new Error("내 서버 목록을 확인하지 못했어요."); }
  const owners = (root.servers as CentralServer[]).filter(server => server.relation === "owner");
  for (const server of owners) {
    if (!server.server_id || !server.registration_epoch || !server.host_key_fingerprint || !server.host_public_key_jwk?.x || server.endpoint?.account_deletion_protocol !== "v1") throw new Error("내 서버 앱을 탈퇴 지원 버전으로 업데이트하고 서버를 연결한 뒤 다시 확인해 주세요.");
  }
  if (!isDesktopWebview() || !owners.length) return { host: null, hasOwnServers: owners.length > 0 };
  const ticket = await requestDesktopAccountDeletionTicket();
  live();
  const own = owners.find(server => server.server_id === ticket.server_id);
  if (!own) return { host: null, hasOwnServers: true };
  if (own.host_key_fingerprint !== ticket.host_key_fingerprint || own.host_public_key_jwk?.x !== ticket.host_public_key_x) throw new Error("이 컴퓨터와 내 서버의 등록 키가 일치하지 않아요.");
  return { host: { server_id: own.server_id, registration_epoch: own.registration_epoch!, fingerprint: ticket.host_key_fingerprint, key: ticket.host_public_key_x }, hasOwnServers: true };
}
/** The local admin owner checks actual host-signed central custody, never proof/receipt possession. */
export async function stopDeletionOwnHost(session: CentralSession, own: OwnDeletionHost, signal: AbortSignal): Promise<void> {
  signal.throwIfAborted();
  if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요.");
  const ticket = await requestDesktopAccountDeletionTicket();
  signal.throwIfAborted();
  if (loadCentralSession()?.token !== session.token || ticket.server_id !== own.server_id || ticket.host_key_fingerprint !== own.fingerprint || ticket.host_public_key_x !== own.key) throw new Error("이 컴퓨터의 서버 등록이 바뀌었어요.");
  const response = await fetch(`${ticket.http_base_url}/api/central-directory/registration-proof`, { method: "POST", headers: { "content-type": "application/json", Authorization: `Bearer ${ticket.ticket}` }, signal,
    body: JSON.stringify({ server_id: own.server_id, account_deletion: true, expected_owner_person_id: session.person.person_id, registration_epoch: own.registration_epoch }) });
  const result = strictRecord(await response.json(), "내 서버 중지");
  if (!response.ok) throw new Error("내 서버의 외부 접속과 계정 연결 중지를 확인하지 못했어요. 계정 탈퇴는 진행하지 않았어요.");
  assertExactKeys(result, ["status"], "내 서버 중지");
  if (result.status !== "account_host_stopped") throw new Error("내 서버 중지 결과를 확인하지 못했어요.");
}
