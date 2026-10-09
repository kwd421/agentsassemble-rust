import { AccountDeletion, parseDeletionServers, type DeletionProgress } from "./accountDeletion";
import { centralSessionFingerprint } from "./googleRegistration";
import { nativeGoogleAuthorization } from "./nativeGoogleAuthorization";
import { isDesktopWebview } from "../desktopBridge";
import { encodeBase64Url } from "../base64Url";
import { isCentralWebEntry, loadCentralSession, signedRequest, parseCentralGoogleHandoff } from "./identity";
import { assertExactKeys, strictRecord } from "../strictJsonContract";

const KEY = "agentsassemble.accountDeletionGoogle.v1";
type Pending = { requestId: string; personId: string; deviceId: string; fingerprint: string | null; verifier: string; state: string; expiresAt: number; progress: DeletionProgress[] | null };
let returnState: { operation: AccountDeletion | null; error: string } | null = null;
const listeners = new Set<() => void>();
export const deletionGoogleReturnSnapshot = () => returnState;
export function subscribeDeletionGoogleReturn(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }
export function clearDeletionGoogleReturn() { returnState = null; for (const listener of listeners) listener(); }

function started(value: unknown, requestId: string) {
  const root = strictRecord(value, "Google 탈퇴 확인");
  assertExactKeys(root, ["handoff_id", "request_id", "authorization_url", "state", "expires_at"], "Google 탈퇴 확인");
  if (root.request_id !== requestId || root.handoff_id !== `goh_${requestId}`) throw new Error("탈퇴 확인 요청이 일치하지 않아요.");
  return parseCentralGoogleHandoff(root);
}
export async function confirmDeletionGoogle(operation: AccountDeletion, signal: AbortSignal): Promise<void> {
  const session = operation.session;
  const live = () => { signal.throwIfAborted(); if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요."); };
  live();
  const flow_kind = isDesktopWebview() ? "native" : "web";
  const begin = async (body: { code_challenge: string; state: string; redirect_uri?: string }) => {
    live();
    const result = await signedRequest<unknown>(session, "/v1/account/deletion-proof", "POST", { kind: "google", flow_kind, request_id: operation.requestId, action: "start", ...body }, signal);
    live(); return started(result, operation.requestId);
  };
  if (flow_kind === "native") {
    const { body } = await nativeGoogleAuthorization(begin, undefined, signal);
    live();
    const proof = await signedRequest<unknown>(session, "/v1/account/deletion-proof", "POST", { kind: "google", flow_kind, request_id: operation.requestId, action: "complete", authorization_code: body.authorization_code, code_verifier: body.code_verifier }, signal);
    live(); operation.acceptGoogleProof(proof); return;
  }
  if (!isCentralWebEntry()) throw new Error("계정 페이지에서 탈퇴를 확인해 주세요.");
  const verifier = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32))), state = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const code_challenge = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  const handoff = await begin({ code_challenge, state });
  const url = new URL(handoff.authorization_url);
  if (handoff.state !== state || url.searchParams.get("redirect_uri") !== `${window.location.origin}/` || url.searchParams.get("code_challenge") !== code_challenge) throw new Error("Google 탈퇴 확인 응답이 일치하지 않아요.");
  const pending: Pending = { requestId: operation.requestId, personId: session.person.person_id, deviceId: session.device_id, fingerprint: await centralSessionFingerprint(), verifier, state, expiresAt: handoff.expires_at, progress: operation.progress };
  live();
  sessionStorage.setItem(KEY, JSON.stringify(pending));
  if (sessionStorage.getItem(KEY) !== JSON.stringify(pending)) throw new Error("탈퇴 확인 요청을 저장하지 못했어요.");
  window.location.assign(url.toString());
}
/** This return owner runs before ordinary OAuth login sanitizes callback parameters. */
export async function completeDeletionGoogleReturn(): Promise<boolean> {
  if (!isCentralWebEntry()) return false;
  const stored = sessionStorage.getItem(KEY);
  if (!stored) return false;
  const url = new URL(window.location.href);
  const returned = url.searchParams.has("code") || url.searchParams.has("error") || url.searchParams.has("state");
  if (!returned) return false;
  window.history.replaceState({}, "", `${url.pathname}?account=settings`);
  sessionStorage.removeItem(KEY);
  let operation: AccountDeletion | null = null;
  let error = "";
  try {
    const pending = JSON.parse(stored) as Pending, session = loadCentralSession();
    if (!session || session.person.person_id !== pending.personId || session.device_id !== pending.deviceId || await centralSessionFingerprint() !== pending.fingerprint) throw new Error("로그인 계정이 바뀌었어요. 처음 계정으로 다시 확인해 주세요.");
    if (!/^[A-Za-z0-9_-]{43}$/.test(pending.requestId) || !/^[A-Za-z0-9_-]{43}$/.test(pending.verifier) || !/^[A-Za-z0-9_-]{43}$/.test(pending.state) || !Number.isSafeInteger(pending.expiresAt) || pending.expiresAt <= Date.now() / 1000) throw new Error("Google 확인 시간이 만료됐어요. 다시 확인해 주세요.");
    operation = new AccountDeletion(session, pending.requestId);
    if (pending.progress !== null) {
      const servers = parseDeletionServers({ servers: pending.progress.map(item => item.server) });
      operation.progress = servers.map((server, index) => ({ server, state: pending.progress![index].state === "removed" ? "removed" : "waiting" }));
    }
    // Preserve confirmed progress even when fresh authentication fails.
    if (url.searchParams.getAll("state").length !== 1 || url.searchParams.get("state") !== pending.state || url.searchParams.has("error") || url.searchParams.getAll("code").length !== 1) throw new Error("Google 탈퇴 확인이 취소됐거나 일치하지 않아요. 다시 확인해 주세요.");
    const proof = await signedRequest<unknown>(session, "/v1/account/deletion-proof", "POST", { kind: "google", flow_kind: "web", action: "complete", request_id: operation.requestId, code_verifier: pending.verifier, authorization_code: url.searchParams.get("code") });
    operation.acceptGoogleProof(proof);
    await operation.inventory(); // recheck signed own-host floor after OAuth navigation
  } catch (reason) { error = reason instanceof Error ? reason.message : "탈퇴 확인을 완료하지 못했어요."; }
  returnState = { operation, error }; for (const listener of listeners) listener();
  return true;
}
