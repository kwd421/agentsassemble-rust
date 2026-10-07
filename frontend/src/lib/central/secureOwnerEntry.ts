import { getOrCreateBrowserCredential, getOrCreateClientId } from "../deviceIdentity";
import { isDesktopWebview } from "../desktopBridge";
import { RemoteTransport } from "../remote/remoteTransport";
import { SECURE_PROTOCOL, type SecureTarget } from "../remote/secureCrypto";
import { installRemoteWorkspace } from "../remote/remoteWorkspace";
import { assertExactKeys, strictRecord } from "../strictJsonContract";
import { isCentralWebEntry, loadCentralSession, signedRequest, type CentralServer } from "./identity";
import { exchangeCentralOwnerSession } from "./ownerWorkspace";

export function secureServerTarget(server: CentralServer): SecureTarget {
  if (!server.endpoint || server.endpoint.status !== "published" || server.endpoint.mode !== "event_secure_v1" ||
      server.endpoint.protocol !== SECURE_PROTOCOL || !server.registration_epoch || !server.host_public_key_jwk) {
    throw new Error("이 서버에 연결할 수 없어요. 서버 앱을 업데이트하고 외부 접속을 켜 주세요.");
  }
  return { server_id: server.server_id, registration_epoch: server.registration_epoch,
    origin: server.endpoint.origin, generation: server.endpoint.generation,
    host_public_key_jwk: server.host_public_key_jwk, host_key_fingerprint: server.host_key_fingerprint };
}

export async function openCentralOwnedServer(server: CentralServer): Promise<void> {
  const session = loadCentralSession();
  if (!session) throw new Error("로그인이 필요해요. 다시 로그인해 주세요.");
  if (server.relation !== "owner" || (!isDesktopWebview() && !isCentralWebEntry())) throw new Error("계정 페이지에서 서버를 열어 주세요.");
  const target = secureServerTarget(server);
  // The direct authenticated handshake is the liveness probe. It writes nothing
  // centrally and the resulting key stays in this trusted entry.
  const transport = await RemoteTransport.connect(target, "owner");
  try {
    const { protocol, client_public_key, channel_id, origin, generation, purpose } = transport.hello;
    const binding = { protocol, client_public_key, channel_id, origin, generation, purpose, registration_epoch: target.registration_epoch };
    const grant = strictRecord(await signedRequest(session, `/v1/servers/${encodeURIComponent(server.server_id)}/connect-grants`, "POST", binding), "서버 접속권");
    assertExactKeys(grant, ["grant_token", "server_id", "expires_at", ...Object.keys(binding)], "서버 접속권");
    if (Object.entries(binding).some(([key, value]) => grant[key] !== value) || grant.server_id !== target.server_id ||
        typeof grant.grant_token !== "string" || !/^aacg1\.[A-Za-z0-9_-]{43}$/.test(grant.grant_token) ||
        !Number.isSafeInteger(grant.expires_at) || Number(grant.expires_at) <= Date.now() / 1000 || loadCentralSession()?.token !== session.token) {
      throw new Error("서버 접속권을 확인하지 못했어요. 다시 연결해 주세요.");
    }
    const deviceToken = getOrCreateBrowserCredential(), clientId = getOrCreateClientId();
    const owner = await exchangeCentralOwnerSession({ grantToken: grant.grant_token, serverId: target.server_id,
      generation, expiresAt: Number(grant.expires_at), hostPublicKeyX: String(target.host_public_key_jwk.x),
      hostKeyFingerprint: target.host_key_fingerprint }, deviceToken, transport);
    if (loadCentralSession()?.token !== session.token || !transport.active) throw new Error("로그인 상태가 바뀌었어요. 다시 연결해 주세요.");
    installRemoteWorkspace({ transport, owner, deviceToken, clientId });
  } catch (error) { transport.close(); throw error; }
}
