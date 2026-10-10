import { encodeBase64Url } from "../base64Url";
import { clearCentralWebGooglePending, completeCentralWebGoogleReturn, startCentralWebGoogle } from "./webGoogle";
import { GoogleRegistrationRequired } from "./googleRegistration";
import { isCentralWebEntry, loadCentralSession } from "./identity";

const KEY = "agentsassemble.memberLoginPopup.v1";
const channelName = (nonce: string) => `${KEY}:${nonce}`;
const failure = () => new Error("로그인을 마치지 못했어요. 초대 창에서 다시 시도해 주세요.");
type Pending = { nonce: string; state: string; expiresAt: number };
function pendingLogin(): Pending {
  const pending = JSON.parse(sessionStorage.getItem(KEY) || "null");
  if (!pending || !/^[A-Za-z0-9_-]{43}$/.test(pending.nonce) || !/^[A-Za-z0-9_-]{43}$/.test(pending.state) ||
      !Number.isSafeInteger(pending.expiresAt) || pending.expiresAt <= Date.now() / 1000) throw failure();
  return pending;
}
export function isMemberLoginPopup() {
  return isCentralWebEntry() && (new URL(window.location.href).searchParams.has("member-login") || sessionStorage.getItem(KEY) !== null);
}

export function loginMemberPopup(signal: AbortSignal): Promise<void> {
  if (!isCentralWebEntry()) return Promise.reject(failure());
  signal.throwIfAborted();
  const nonce = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  // This executes synchronously in the original button click, before any await.
  const channel = new BroadcastChannel(channelName(nonce));
  const popup = window.open(`/?member-login=${nonce}`, "_blank", "popup,width=520,height=700");
  if (!popup) { channel.close(); return Promise.reject(new Error("로그인 창이 차단됐어요. 팝업을 허용한 뒤 Google로 다시 시도해 주세요.")); }
  return new Promise((resolve, reject) => {
    let state = "", returned = false, finished = false;
    let timer = setTimeout(() => finish(failure()), 300_000);
    const finish = (error?: Error) => {
      if (finished) return; finished = true; clearTimeout(timer);
      channel.postMessage({ type: "member-login-ended", nonce, state }); channel.close();
      window.removeEventListener("message", message); window.removeEventListener("focus", focus);
      signal.removeEventListener("abort", abort);
      if (!error) popup.close();
      if (error) reject(error); else resolve();
    };
    const message = (event: MessageEvent) => {
      if (event.origin !== window.location.origin || event.source !== popup || event.data?.nonce !== nonce) return;
      const data = event.data;
      if (data.type === "member-login-started" && !state && /^[A-Za-z0-9_-]{43}$/.test(data.state) && Number.isSafeInteger(data.expiresAt) && data.expiresAt > Date.now() / 1000) {
        state = data.state; clearTimeout(timer);
        timer = setTimeout(() => finish(failure()), Math.min(300_000, data.expiresAt * 1000 - Date.now()));
        channel.postMessage({ type: "member-login-ready", nonce, state });
      }
    };
    channel.onmessage = event => {
      const data = event.data;
      if (!state || data?.nonce !== nonce || data.state !== state) return;
      if (data.type === "member-login-returned") { returned = true; channel.postMessage({ type: "member-login-ready", nonce, state }); }
      else if (data.type === "member-login-complete" && returned && loadCentralSession()) finish();
      else if (data.type === "member-login-failed") finish(failure());
    };
    const abort = () => { finish(failure()); popup.close(); };
    // COOP can report closed=true after navigation although the OAuth window lives.
    const focus = () => { if (!state && popup.closed) finish(failure()); };
    window.addEventListener("message", message); window.addEventListener("focus", focus);
    signal.addEventListener("abort", abort, { once: true });
  });
}

async function acknowledge(pending: Pending, notify: (channel: BroadcastChannel) => void, signal: AbortSignal) {
  signal.throwIfAborted();
  const channel = new BroadcastChannel(channelName(pending.nonce));
  try {
    await new Promise<void>((resolve, reject) => {
      const finish = (error?: Error) => { clearTimeout(timer); signal.removeEventListener("abort", abort); error ? reject(error) : resolve(); };
      const abort = () => finish(failure());
      const timer = setTimeout(() => finish(failure()), Math.max(0, Math.min(10_000, pending.expiresAt * 1000 - Date.now())));
      signal.addEventListener("abort", abort, { once: true });
      channel.onmessage = event => {
        const data = event.data;
        if (data?.nonce === pending.nonce && data.state === pending.state) {
          if (data.type === "member-login-ready") finish();
          else if (data.type === "member-login-ended") finish(failure());
        }
      };
      notify(channel);
    });
  } finally { channel.close(); }
}

export async function registerMemberLoginPopup(registration: GoogleRegistrationRequired, signal: AbortSignal) {
  const pending = pendingLogin();
  await acknowledge(pending, channel => channel.postMessage({ type: "member-login-returned", ...pending }), signal);
  try {
    await registration.register(signal);
    signal.throwIfAborted();
    finishMemberLoginPopup(true);
  } catch (error) { finishMemberLoginPopup(false); throw error; }
}

export function finishMemberLoginPopup(success: boolean) {
  try {
    const stored = sessionStorage.getItem(KEY);
    if (!stored) return;
    let pending: Pending;
    try { pending = pendingLogin(); } catch { return; }
    const channel = new BroadcastChannel(channelName(pending.nonce));
    channel.postMessage({ type: success ? "member-login-complete" : "member-login-failed", nonce: pending.nonce, state: pending.state });
    channel.close();
  } finally { sessionStorage.removeItem(KEY); clearCentralWebGooglePending(); }
}

export async function runMemberLoginPopup(signal: AbortSignal): Promise<void> {
  const url = new URL(window.location.href);
  const nonce = url.searchParams.get("member-login");
  window.history.replaceState({}, "", "/");
  try {
    if (nonce) {
      window.history.replaceState({}, "", "/");
      if (!isCentralWebEntry() || !window.opener || !/^[A-Za-z0-9_-]{43}$/.test(nonce)) throw failure();
      await startCentralWebGoogle(signal, async handoff => {
        const pending = { nonce, state: handoff.state, expiresAt: Math.min(handoff.expires_at, Math.floor(Date.now() / 1000) + 300) };
        sessionStorage.setItem(KEY, JSON.stringify(pending));
        await acknowledge(pending, () => window.opener.postMessage({ type: "member-login-started", ...pending }, window.location.origin), signal);
      });
      return;
    }
    if (!isCentralWebEntry()) throw failure();
    const pending = pendingLogin();
    if (url.searchParams.getAll("state").length !== 1 || url.searchParams.get("state") !== pending.state) throw failure();
    await acknowledge(pending, channel => channel.postMessage({ type: "member-login-returned", ...pending }), signal);
    await completeCentralWebGoogleReturn(url, signal);
    signal.throwIfAborted();
    if (!loadCentralSession()) throw failure();
    finishMemberLoginPopup(true);
  } catch (error) {
    window.history.replaceState({}, "", "/");
    if (!(error instanceof GoogleRegistrationRequired)) {
      finishMemberLoginPopup(false);
    }
    throw error;
  }
}
