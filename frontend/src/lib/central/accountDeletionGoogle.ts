import { AccountDeletion } from "./accountDeletion";
import { nativeGoogleAuthorization } from "./nativeGoogleAuthorization";
import { authorizeDeletionGooglePopup } from "./deletionGooglePopup";
import { isDesktopWebview } from "../desktopBridge";
import { isCentralWebEntry, loadCentralSession, signedRequest, parseCentralGoogleHandoff } from "./identity";
import { assertExactKeys, strictRecord } from "../strictJsonContract";

export async function confirmDeletionGoogle(operation: AccountDeletion, signal: AbortSignal): Promise<void> {
  const session = operation.session;
  const live = () => { signal.throwIfAborted(); if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요."); };
  live();
  const flow_kind = isDesktopWebview() ? "native" : "web";
  const begin = async (body: { code_challenge: string; state: string; redirect_uri?: string }, requestSignal = signal) => {
    live();
    const result = await signedRequest<unknown>(session, "/v1/account/deletion-proof", "POST", { kind: "google", flow_kind, request_id: operation.requestId, action: "start", ...body }, requestSignal);
    live();
    const root = strictRecord(result, "Google 탈퇴 확인");
    assertExactKeys(root, ["handoff_id", "request_id", "authorization_url", "state", "expires_at"], "Google 탈퇴 확인");
    if (root.request_id !== operation.requestId || root.handoff_id !== `goh_${operation.requestId}`) throw new Error("탈퇴 확인 요청이 일치하지 않아요.");
    return parseCentralGoogleHandoff(root);
  };
  if (flow_kind === "web" && !isCentralWebEntry()) throw new Error("계정 페이지에서 탈퇴를 확인해 주세요.");
  const body = flow_kind === "native" ? (await nativeGoogleAuthorization(begin, undefined, signal)).body
    : await authorizeDeletionGooglePopup(begin, signal);
  live();
  const proof = await signedRequest<unknown>(session, "/v1/account/deletion-proof", "POST", { kind: "google", flow_kind, request_id: operation.requestId, action: "complete", authorization_code: body.authorization_code, code_verifier: body.code_verifier }, signal);
  live(); operation.acceptGoogleProof(proof);
}
