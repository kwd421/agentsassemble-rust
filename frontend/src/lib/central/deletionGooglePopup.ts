import { encodeBase64Url } from "../base64Url";
import { isCentralWebEntry, type CentralGoogleHandoff } from "./identity";

const KEY = "agentsassemble.deletionGooglePopup.v1";
type Pending = { state: string; expiresAt: number };
const channelName = (state: string) => `${KEY}:${state}`;
const failure = () => new Error("Google 확인을 마치지 못했어요. 다시 시도해 주세요.");
export const isDeletionGooglePopup = () => isCentralWebEntry() && sessionStorage.getItem(KEY) !== null;

/** Open in the click's synchronous stack; COOP return uses the correlated central-origin channel. */
export async function authorizeDeletionGooglePopup(begin: (body: { state: string; code_challenge: string }, signal: AbortSignal) => Promise<CentralGoogleHandoff>, signal: AbortSignal) {
  signal.throwIfAborted();
  const popup = window.open("about:blank", "_blank", "popup,width=520,height=700");
  if (!popup) throw new Error("Google 확인 창이 차단됐어요. 팝업을 허용한 뒤 다시 시도해 주세요.");
  const deadline = Math.floor(Date.now() / 1000) + 600;
  signal = AbortSignal.any([signal, AbortSignal.timeout(600_000)]);
  const state = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const verifier = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const channel = new BroadcastChannel(channelName(state));
  let rejectStart!: () => void;
  try {
    const code_challenge = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
    signal.throwIfAborted();
    const aborted = new Promise<never>((_, reject) => { rejectStart = () => reject(failure()); signal.addEventListener("abort", rejectStart, { once: true }); });
    const handoff = await Promise.race([begin({ state, code_challenge }, signal), aborted]);
    signal.removeEventListener("abort", rejectStart);
    signal.throwIfAborted();
    const url = new URL(handoff.authorization_url);
    if (handoff.state !== state || url.searchParams.get("redirect_uri") !== `${window.location.origin}/` || url.searchParams.get("code_challenge") !== code_challenge) throw failure();
    const expiresAt = Math.min(handoff.expires_at, deadline);
    const pending: Pending = { state, expiresAt };
    popup.sessionStorage.setItem(KEY, JSON.stringify(pending));
    if (popup.sessionStorage.getItem(KEY) !== JSON.stringify(pending)) throw failure();
    const authorization_code = await new Promise<string>((resolve, reject) => {
      let ended = false;
      const finish = (code?: string) => {
        if (ended) return; ended = true;
        clearTimeout(timer); signal.removeEventListener("abort", abort); window.removeEventListener("pagehide", abort);
        code ? resolve(code) : reject(failure());
      };
      const abort = () => finish();
      const timer = setTimeout(abort, Math.max(0, expiresAt * 1000 - Date.now()));
      signal.addEventListener("abort", abort, { once: true }); window.addEventListener("pagehide", abort, { once: true });
      channel.onmessage = event => {
        const data = event.data;
        if (data?.state !== state) return;
        if (data.type === "deletion-google-return" && typeof data.code === "string" && data.code.length > 0 && data.code.length <= 4096) {
          channel.postMessage({ type: "deletion-google-accepted", state }); finish(data.code);
        } else if (data.type === "deletion-google-cancel") finish();
      };
      popup.location.replace(url.toString());
      if (signal.aborted) abort();
    });
    return { authorization_code, code_verifier: verifier };
  } finally { if (rejectStart) signal.removeEventListener("abort", rejectStart); channel.close(); popup.close(); }
}

/** Callback transports only the one authorization code; the original operation owns proof exchange. */
export async function returnDeletionGooglePopup(signal: AbortSignal): Promise<void> {
  const url = new URL(window.location.href);
  window.history.replaceState({}, "", "/");
  const raw = sessionStorage.getItem(KEY); sessionStorage.removeItem(KEY);
  if (!isCentralWebEntry() || !raw) throw failure();
  const pending = JSON.parse(raw) as Pending;
  if (!/^[A-Za-z0-9_-]{43}$/.test(pending.state) || !Number.isSafeInteger(pending.expiresAt) || pending.expiresAt <= Date.now() / 1000) throw failure();
  const channel = new BroadcastChannel(channelName(pending.state));
  try {
    signal.throwIfAborted();
    if (url.searchParams.getAll("state").length !== 1 || url.searchParams.get("state") !== pending.state || url.searchParams.has("error") || url.searchParams.getAll("code").length !== 1) {
      channel.postMessage({ type: "deletion-google-cancel", state: pending.state }); throw failure();
    }
    await new Promise<void>((resolve, reject) => {
      const finish = (ok: boolean) => { clearTimeout(timer); signal.removeEventListener("abort", abort); ok ? resolve() : reject(failure()); };
      const abort = () => finish(false);
      const timer = setTimeout(abort, 10_000);
      signal.addEventListener("abort", abort, { once: true });
      channel.onmessage = event => { if (event.data?.type === "deletion-google-accepted" && event.data.state === pending.state) finish(true); };
      channel.postMessage({ type: "deletion-google-return", state: pending.state, code: url.searchParams.get("code") });
    });
  } finally { channel.close(); }
}
