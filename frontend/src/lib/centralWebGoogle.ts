import { authDeviceBody, isCentralWebEntry, parseCentralGoogleHandoff, saveSession, unsignedPost, type CentralPerson, type CentralSession } from "./centralIdentity";
import { encodeBase64Url } from "./base64Url";

const PENDING_KEY = "agentsassemble.centralWebGoogle.v1";
type PendingLogin = {
  handoffId: string;
  verifier: string;
  state: string;
  expiresAt: number;
  authorizationCode?: string;
};

export async function startCentralWebGoogle(signal: AbortSignal): Promise<void> {
  if (!isCentralWebEntry()) throw new Error("중앙 계정 페이지에서 로그인해 주세요.");
  const verifier = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const state = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  const started = parseCentralGoogleHandoff(await unsignedPost<unknown>(
    "/v1/auth/google/web/start", { ...await authDeviceBody(), code_challenge: challenge, state }, signal
  ));
  const url = new URL(started.authorization_url);
  if (started.state !== state || url.searchParams.get("redirect_uri") !== `${window.location.origin}/` ||
      url.searchParams.get("code_challenge") !== challenge || started.expires_at <= Date.now() / 1000) {
    throw new Error("웹 로그인 응답을 확인하지 못했습니다.");
  }
  if (signal.aborted) throw new DOMException("Google 로그인을 취소했습니다.", "AbortError");
  const pending: PendingLogin = { handoffId: started.handoff_id, verifier, state, expiresAt: started.expires_at };
  // The verifier stays in this tab on the central origin, never in a URL or room host.
  sessionStorage.setItem(PENDING_KEY, JSON.stringify(pending));
  window.location.assign(url.toString());
}

let completion: Promise<void> | undefined;
export function completeCentralWebGoogleReturn(): Promise<void> {
  if (!isCentralWebEntry()) return Promise.resolve();
  if (completion) return completion;
  completion = completeReturn().finally(() => { completion = undefined; });
  return completion;
}

async function completeReturn(): Promise<void> {
  const url = new URL(window.location.href);
  const returned = url.searchParams.has("code") || url.searchParams.has("error") || url.searchParams.has("state");
  if (returned) window.history.replaceState({}, "", url.pathname);
  const stored = sessionStorage.getItem(PENDING_KEY);
  if (!stored) {
    if (returned) throw new Error("이 창에서 시작한 Google 로그인이 아닙니다. 다시 로그인해 주세요.");
    return;
  }
  let pending: PendingLogin;
  try { pending = JSON.parse(stored) as PendingLogin; }
  catch { sessionStorage.removeItem(PENDING_KEY); throw new Error("저장된 로그인 요청을 읽지 못했습니다. 다시 로그인해 주세요."); }
  if (!/^goh_[A-Za-z0-9_-]+$/.test(pending.handoffId) ||
      !/^[A-Za-z0-9_-]{43}$/.test(pending.verifier) || !/^[A-Za-z0-9_-]{43}$/.test(pending.state) ||
      !Number.isSafeInteger(pending.expiresAt) || pending.expiresAt <= Date.now() / 1000) {
    sessionStorage.removeItem(PENDING_KEY);
    throw new Error("Google 로그인 요청이 만료됐습니다. 다시 로그인해 주세요.");
  }
  if (returned) {
    if (url.searchParams.getAll("state").length !== 1 || url.searchParams.get("state") !== pending.state) {
      throw new Error("Google 로그인 요청이 일치하지 않습니다. 다시 로그인해 주세요.");
    }
    if (url.searchParams.has("error")) {
      sessionStorage.removeItem(PENDING_KEY);
      throw new Error("Google 로그인이 취소되었거나 거부되었습니다.");
    }
    if (url.searchParams.getAll("code").length !== 1) throw new Error("Google 로그인 응답을 확인하지 못했습니다.");
    pending.authorizationCode = url.searchParams.get("code") || "";
    sessionStorage.setItem(PENDING_KEY, JSON.stringify(pending));
  }
  if (!pending.authorizationCode) return;
  try {
    const result = await unsignedPost<{ person: CentralPerson; session: Omit<CentralSession, "person"> }>(
      "/v1/auth/google/web/complete", { handoff_id: pending.handoffId, code_verifier: pending.verifier, authorization_code: pending.authorizationCode }
    );
    saveSession(result);
  } finally {
    // A Google code is single-use; a failed exchange exposes a fresh login action.
    sessionStorage.removeItem(PENDING_KEY);
  }
}
