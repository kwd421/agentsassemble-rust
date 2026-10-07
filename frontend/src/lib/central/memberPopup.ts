import { encodeBase64Url } from "../base64Url";
import { completeCentralWebGoogleReturn, startCentralWebGoogle } from "./webGoogle";
import { isCentralWebEntry, loadCentralSession } from "./identity";

const KEY = "agentsassemble.memberLoginPopup.v1";
const failure = () => new Error("로그인을 마치지 못했어요. 원래 초대 링크를 다시 열어 주세요.");
export function isMemberLoginPopup() {
  return isCentralWebEntry() && (new URL(window.location.href).searchParams.has("member-login") || sessionStorage.getItem(KEY) !== null);
}

export function loginMemberPopup(signal: AbortSignal): Promise<void> {
  if (!isCentralWebEntry()) return Promise.reject(failure());
  signal.throwIfAborted();
  const nonce = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const popup = window.open(`/?member-login=${nonce}`, "_blank", "popup,width=520,height=700");
  if (!popup) return Promise.reject(failure());
  return new Promise((resolve, reject) => {
    let state = "", finished = false;
    let timer = setTimeout(() => finish(failure()), 300_000);
    const finish = (error?: Error) => {
      if (finished) return; finished = true; clearTimeout(timer);
      window.removeEventListener("message", message); window.removeEventListener("focus", focus);
      signal.removeEventListener("abort", abort); popup.close();
      if (error) reject(error); else resolve();
    };
    const message = (event: MessageEvent) => {
      if (event.origin !== window.location.origin || event.source !== popup || event.data?.nonce !== nonce) return;
      const data = event.data;
      if (data.type === "member-login-started" && !state && /^[A-Za-z0-9_-]{43}$/.test(data.state) && Number.isSafeInteger(data.expiresAt)) {
        state = data.state; clearTimeout(timer);
        timer = setTimeout(() => finish(failure()), Math.max(0, Math.min(300_000, data.expiresAt * 1000 - Date.now())));
      } else if (data.type === "member-login-complete" && state && data.state === state && loadCentralSession()) finish();
      else if (data.type === "member-login-failed") finish(failure());
    };
    const abort = () => finish(failure());
    const focus = () => { if (popup.closed) finish(failure()); };
    window.addEventListener("message", message); window.addEventListener("focus", focus);
    signal.addEventListener("abort", abort, { once: true });
  });
}

export async function runMemberLoginPopup(): Promise<void> {
  if (!isMemberLoginPopup() || !window.opener) throw failure();
  const url = new URL(window.location.href);
  const nonce = url.searchParams.get("member-login");
  if (nonce) {
    window.history.replaceState({}, "", "/");
    if (!/^[A-Za-z0-9_-]{43}$/.test(nonce)) throw failure();
    await startCentralWebGoogle(AbortSignal.timeout(300_000), handoff => {
      sessionStorage.setItem(KEY, JSON.stringify({ nonce, state: handoff.state, expiresAt: Math.min(handoff.expires_at, Math.floor(Date.now() / 1000) + 300) }));
      window.opener.postMessage({ type: "member-login-started", nonce, state: handoff.state, expiresAt: handoff.expires_at }, window.location.origin);
    });
    return;
  }
  const pending = JSON.parse(sessionStorage.getItem(KEY) || "null");
  try {
    if (!pending || pending.expiresAt <= Date.now() / 1000 || url.searchParams.get("state") !== pending.state) throw failure();
    await completeCentralWebGoogleReturn();
    if (!loadCentralSession()) throw failure();
    window.opener.postMessage({ type: "member-login-complete", nonce: pending.nonce, state: pending.state }, window.location.origin);
  } catch (error) {
    window.opener.postMessage({ type: "member-login-failed", nonce: pending?.nonce }, window.location.origin); throw error;
  } finally { sessionStorage.removeItem(KEY); }
}
