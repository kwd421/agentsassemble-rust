import { deletionOwnHost, stopDeletionOwnHost, type OwnDeletionHost } from "./accountDeletionHost";
import { clearCentralSession, clearPendingCentralRecoveryCode, loadCentralSession, signedRequest, unsignedPost, type CentralSession, CENTRAL_SESSION_CHANGED_EVENT, CENTRAL_SESSION_CLEARED_EVENT } from "./identity";
import { encodeBase64Url } from "../base64Url";
import { RemoteTransport } from "../remote/remoteTransport";
import { SECURE_PROTOCOL, type SecureTarget } from "../remote/secureCrypto";
import { getOrCreateBrowserCredential } from "../deviceIdentity";
import { parseMemberChallenge } from "./memberConnect";
import { assertExactKeys, requiredString, strictRecord } from "../strictJsonContract";

export type DeletionServer = { server_id: string; registration_epoch: string; name: string;
  user_hidden: boolean; host_state: string; host_key_fingerprint: string; host_public_key_jwk: JsonWebKey | null;
  endpoint: null | { origin: string; generation: number; status: string; mode?: string; protocol?: string; account_deletion_protocol?: string | null } };
export type DeletionProgress = { server: DeletionServer; state: "waiting" | "working" | "removed" | "skipped"; reason?: string };
export type DeletionProof = { request_id: string; proof: string; expires_at: number };
export type DeletionResult = { name: string; server_id: string; registration_epoch: string; state: "removed" | "skipped"; reason: string };
export type DeletionReceipt = { person_id: string; request_id: string; receipt: string; expires_at: number; results: DeletionResult[] };
const RECEIPT_KEY = "agentsassemble.accountDeletionReceipt.v1";
const token = () => encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
const invalid = () => new Error("탈퇴 응답을 확인하지 못했어요. 다시 확인해 주세요.");

export function parseDeletionServers(value: unknown): DeletionServer[] {
  const root = strictRecord(value, "탈퇴 서버 목록");
  assertExactKeys(root, ["servers"], "탈퇴 서버 목록");
  if (!Array.isArray(root.servers) || root.servers.length > 512 || new TextEncoder().encode(JSON.stringify(root)).length > 512 * 1024) throw invalid();
  const seen = new Set<string>();
  return root.servers.map(value => {
    const r = strictRecord(value, "탈퇴 서버");
    assertExactKeys(r, ["server_id", "registration_epoch", "name", "user_hidden", "host_state", "host_key_fingerprint", "host_public_key_jwk", "endpoint"], "탈퇴 서버");
    const server_id = requiredString(r, "server_id", "탈퇴 서버"), registration_epoch = requiredString(r, "registration_epoch", "탈퇴 서버");
    const name = requiredString(r, "name", "탈퇴 서버"), host_state = requiredString(r, "host_state", "탈퇴 서버");
    const key = JSON.stringify([server_id, registration_epoch]);
    if (seen.has(key) || server_id.length > 200 || registration_epoch.length > 200 || name.length > 256 || typeof r.user_hidden !== "boolean" || typeof r.host_key_fingerprint !== "string") throw invalid();
    seen.add(key);
    const host_public_key_jwk = r.host_public_key_jwk === null ? null : strictRecord(r.host_public_key_jwk, "서버 키") as JsonWebKey;
    let endpoint: DeletionServer["endpoint"] = null;
    if (r.endpoint !== null) {
      const e = strictRecord(r.endpoint, "서버 주소");
      assertExactKeys(e, ["origin", "generation", "status"], "서버 주소", ["mode", "protocol", "account_deletion_protocol"]);
      if (typeof e.origin !== "string" || e.origin.length > 2048 || typeof e.status !== "string" || typeof e.generation !== "number" || !Number.isSafeInteger(e.generation) || e.generation < 1 ||
        (e.mode !== undefined && typeof e.mode !== "string") || (e.protocol !== undefined && typeof e.protocol !== "string") ||
        (e.account_deletion_protocol !== undefined && e.account_deletion_protocol !== null && e.account_deletion_protocol !== "v1")) throw invalid();
      endpoint = e as DeletionServer["endpoint"];
    }
    return { server_id, registration_epoch, name, host_state, user_hidden: r.user_hidden, host_key_fingerprint: r.host_key_fingerprint, host_public_key_jwk, endpoint };
  });
}

export function parseDeletionProof(value: unknown, requestId: string): DeletionProof {
  const r = strictRecord(value, "탈퇴 확인");
  assertExactKeys(r, ["request_id", "proof", "expires_at"], "탈퇴 확인");
  if (r.request_id !== requestId || typeof r.proof !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(r.proof) ||
    typeof r.expires_at !== "number" || !Number.isSafeInteger(r.expires_at) || r.expires_at <= Date.now() / 1000 || r.expires_at > Date.now() / 1000 + 301) throw invalid();
  return r as DeletionProof;
}

export function loadDeletionReceipt(): DeletionReceipt | null {
  const raw = sessionStorage.getItem(RECEIPT_KEY);
  if (!raw) return null;
  const r = strictRecord(JSON.parse(raw), "탈퇴 결과 확인");
  assertExactKeys(r, ["person_id", "request_id", "receipt", "expires_at", "results"], "탈퇴 결과 확인");
  if (typeof r.person_id !== "string" || !r.person_id || typeof r.request_id !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(r.request_id) ||
    typeof r.receipt !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(r.receipt) || typeof r.expires_at !== "number" || !Number.isSafeInteger(r.expires_at)) throw invalid();
  if (!Array.isArray(r.results) || r.results.length > 512) throw invalid();
  for (const value of r.results) {
    const row = strictRecord(value, "기기별 정리 결과");
    assertExactKeys(row, ["name", "server_id", "registration_epoch", "state", "reason"], "기기별 정리 결과");
    if (!["removed", "skipped"].includes(String(row.state)) || ["name", "server_id", "registration_epoch", "reason"].some(key => typeof row[key] !== "string" || (row[key] as string).length > 512)) throw invalid();
  }
  return r as DeletionReceipt;
}

/** Explicit result lookup uses only the one receipt; no login, registration or polling. */
export async function checkDeletionReceipt(signal?: AbortSignal): Promise<"account_deleted" | "unknown"> {
  const receipt = loadDeletionReceipt();
  if (!receipt || receipt.expires_at <= Date.now() / 1000) return "unknown";
  try {
    const r = strictRecord(await unsignedPost<unknown>(`/v1/account-deletions/${receipt.request_id}/status`, { person_id: receipt.person_id, receipt: receipt.receipt }, signal), "탈퇴 결과");
    assertExactKeys(r, ["status"], "탈퇴 결과");
    if (r.status !== "account_deleted") throw invalid();
    if (loadCentralSession()?.person.person_id === receipt.person_id) { clearCentralSession(); clearPendingCentralRecoveryCode(); }
    return "account_deleted";
  } catch (error) {
    if (error instanceof Error && "status" in error && error.status === 404) return "unknown";
    throw error;
  }
}

/** One device-owned operation. Proof stays in RAM; only the receipt survives a lost final response. */
export class AccountDeletion {
  readonly requestId: string;
  readonly session: CentralSession;
  progress: DeletionProgress[] | null = null;
  ownHost: OwnDeletionHost | null = null;
  ownStopped = false;
  hasOwnServers = false;
  get hasFreshProof() { return Boolean(this.proof && this.proof.expires_at > Date.now() / 1000); }
  private proof: DeletionProof | null = null;
  private busy = false;
  private inventoryOk = false;
  get inventoryReady() { return this.inventoryOk; }
  private receipt: DeletionReceipt | null = null;
  private controller = new AbortController();
  constructor(session: CentralSession, requestId = token()) { if (!/^[A-Za-z0-9_-]{43}$/.test(requestId)) throw invalid(); this.requestId = requestId; this.session = session; window.addEventListener(CENTRAL_SESSION_CHANGED_EVENT, this.identityChanged); window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, this.identityChanged); window.addEventListener("storage", this.identityChanged); }
  private identityChanged = () => { if (loadCentralSession()?.token !== this.session.token) this.cancel(); };
  cancel() { this.controller.abort(); this.proof = null; window.removeEventListener(CENTRAL_SESSION_CHANGED_EVENT, this.identityChanged); window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, this.identityChanged); window.removeEventListener("storage", this.identityChanged); }
  private live() {
    this.controller.signal.throwIfAborted();
    if (loadCentralSession()?.token !== this.session.token) { this.cancel(); throw new Error("로그인 계정이 바뀌었어요. 처음부터 다시 확인해 주세요."); }
  }
  private freshProof(): DeletionProof {
    this.live();
    if (!this.proof || this.proof.expires_at <= Date.now() / 1000) throw new Error("확인 시간이 만료됐어요. 같은 계정으로 다시 확인해 주세요. 완료한 서버 정리는 다시 하지 않아요.");
    return this.proof;
  }
  async confirmGuest(recoveryCode: string) {
    this.live();
    const result = await signedRequest<unknown>(this.session, "/v1/account/deletion-proof", "POST", { kind: "guest", request_id: this.requestId, recovery_code: recoveryCode }, this.controller.signal);
    this.live(); this.proof = parseDeletionProof(result, this.requestId);
  }
  acceptGoogleProof(proof: unknown) { this.live(); this.proof = parseDeletionProof(proof, this.requestId); }
  async inventory() {
    this.inventoryOk = false;
    this.live();
    const own = await deletionOwnHost(this.session, this.controller.signal);
    const value = await signedRequest<unknown>(this.session, "/v1/account/deletion-servers", "POST", {}, this.controller.signal);
    this.live();
    const previous = this.progress;
    this.ownHost = own.host; this.hasOwnServers = own.hasOwnServers;
    this.progress = parseDeletionServers(value).map(server => previous?.find(item => item.server.server_id === server.server_id && item.server.registration_epoch === server.registration_epoch && item.state === "removed") ?? { server, state: "waiting" });
    this.inventoryOk = true;
  }
  async removeReachable(confirmation: string, changed: () => void) {
    if (confirmation !== "탈퇴" || this.busy || !this.progress || !this.inventoryOk) throw new Error("‘탈퇴’를 입력하고 서버 목록을 확인해 주세요.");
    this.freshProof(); this.busy = true;
    try {
      if (this.ownHost && !this.ownStopped) { await stopDeletionOwnHost(this.session, this.ownHost, AbortSignal.any([this.controller.signal, AbortSignal.timeout(30_000)])); this.ownStopped = true; changed(); }
      for (const item of this.progress) {
        if (item.state === "removed") continue;
        this.freshProof(); item.state = "working"; delete item.reason; changed();
        const e = item.server.endpoint;
        if (!e || e.status !== "published" || item.server.host_state !== "active") {
          item.state = "skipped"; item.reason = "지금 연결할 수 없어요.";
        } else if (e.account_deletion_protocol !== "v1" || e.protocol !== SECURE_PROTOCOL || e.mode !== "event_secure_v1" || !item.server.host_public_key_jwk) {
          item.state = "skipped"; item.reason = "이 서버는 탈퇴 정리를 지원하지 않아요.";
        } else {
          try { await this.removeOne(item.server); item.state = "removed"; }
          catch (error) {
            this.freshProof(); item.state = "skipped";
            item.reason = error instanceof Error ? error.message : "서버 정리 완료를 확인하지 못했어요.";
          }
        }
        changed();
      }
    } finally { this.busy = false; }
  }
  private async removeOne(server: DeletionServer) {
    const endpoint = server.endpoint!;
    const target: SecureTarget = { server_id: server.server_id, registration_epoch: server.registration_epoch,
      origin: endpoint.origin, generation: endpoint.generation, host_public_key_jwk: server.host_public_key_jwk!, host_key_fingerprint: server.host_key_fingerprint };
    const bounded = AbortSignal.any([this.controller.signal, AbortSignal.timeout(30_000)]);
    const transport = await RemoteTransport.connect(target, "account_deletion", bounded);
    try {
      const headers = { "content-type": "application/json", "x-device-token": getOrCreateBrowserCredential(server) };
      const response = await transport.fetch("/api/account-removal/challenge", { method: "POST", headers, body: "{}", signal: bounded });
      if (!response.ok) throw new Error("서버에서 탈퇴 정리를 시작하지 못했어요.");
      const challenge = parseMemberChallenge(await response.json());
      if (challenge.server_id !== target.server_id || challenge.registration_epoch !== target.registration_epoch) throw invalid();
      const proof = this.freshProof(), hello = transport.hello;
      const grant = strictRecord(await signedRequest<unknown>(this.session, `/v1/servers/${encodeURIComponent(target.server_id)}/account-deletion-grants`, "POST", {
        protocol: hello.protocol, purpose: "account_deletion", registration_epoch: target.registration_epoch,
        client_public_key: hello.client_public_key, channel_id: hello.channel_id, origin: hello.origin, generation: hello.generation,
        challenge_hash: challenge.challenge_hash, request_id: this.requestId, proof: proof.proof,
      }, bounded), "탈퇴 정리 권한");
      assertExactKeys(grant, ["grant_token", "server_id", "registration_epoch", "expires_at", "endpoint_origin", "endpoint_generation", "protocol", "client_public_key", "channel_id", "purpose", "request_id"], "탈퇴 정리 권한");
      this.live();
      if (grant.server_id !== target.server_id || grant.registration_epoch !== target.registration_epoch || grant.endpoint_origin !== target.origin || grant.endpoint_generation !== target.generation ||
        grant.protocol !== hello.protocol || grant.client_public_key !== hello.client_public_key || grant.channel_id !== hello.channel_id || grant.purpose !== "account_deletion" || grant.request_id !== this.requestId ||
        typeof grant.grant_token !== "string" || !/^aadg1\.[A-Za-z0-9_-]{43}$/.test(grant.grant_token) || typeof grant.expires_at !== "number" || !Number.isSafeInteger(grant.expires_at) || grant.expires_at <= Date.now() / 1000 || grant.expires_at > proof.expires_at) throw invalid();
      const result = await transport.fetch("/api/account-removal/execute", { method: "POST", headers,
        body: JSON.stringify({ challenge_id: challenge.challenge_id, grant_token: grant.grant_token, request_id: this.requestId }), signal: bounded });
      if (!result.ok) throw new Error("서버 정리 완료를 확인하지 못했어요. 이름이 남을 수 있어요.");
      const r = strictRecord(await result.json(), "서버 탈퇴 정리");
      assertExactKeys(r, ["status"], "서버 탈퇴 정리");
      if (r.status !== "account_removed") throw invalid();
    } finally { transport.close(); }
  }
  async disable(confirmation: string): Promise<void> {
    if (confirmation !== "탈퇴" || this.busy || !this.progress || !this.inventoryOk || this.progress.some(item => item.state === "waiting" || item.state === "working") || (this.ownHost && !this.ownStopped)) throw new Error("서버별 정리 결과를 먼저 확인해 주세요.");
    const proof = this.freshProof(); this.busy = true;
    const receipt = this.receipt ??= { person_id: this.session.person.person_id, request_id: this.requestId, receipt: token(), expires_at: Math.floor(Date.now() / 1000) + 86400, results: this.progress.map(item => ({ name: item.server.name, server_id: item.server.server_id, registration_epoch: item.server.registration_epoch, state: item.state as "removed" | "skipped", reason: (item.reason ?? "").slice(0, 512) })) };
    // Persist before dispatch. If durable local receipt storage fails, no disable is sent.
    try {
      sessionStorage.setItem(RECEIPT_KEY, JSON.stringify(receipt));
      if (sessionStorage.getItem(RECEIPT_KEY) !== JSON.stringify(receipt)) throw invalid();
      const r = strictRecord(await signedRequest<unknown>(this.session, "/v1/account", "DELETE", {
        confirmation: `delete:${this.session.person.person_id}`, request_id: this.requestId, proof: proof.proof, receipt: receipt.receipt,
      }, AbortSignal.any([this.controller.signal, AbortSignal.timeout(15_000)])), "계정 탈퇴");
      assertExactKeys(r, ["status", "request_id", "receipt_expires_at"], "계정 탈퇴");
      if (r.status !== "account_deleted" || r.request_id !== this.requestId || typeof r.receipt_expires_at !== "number" || !Number.isSafeInteger(r.receipt_expires_at) || r.receipt_expires_at <= Date.now() / 1000 || r.receipt_expires_at > Date.now() / 1000 + 86401) throw invalid();
      if (loadCentralSession()?.token === this.session.token) { clearCentralSession(); clearPendingCentralRecoveryCode(); }
      this.proof = null;
    } finally { this.busy = false; }
  }
}
