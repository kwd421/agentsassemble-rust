import { controlDesktopCentralLogin, openDesktopCentralGoogleLogin } from "../desktopBridge";
import { encodeBase64Url } from "../base64Url";
import { parseCentralGoogleHandoff } from "./identity";

export type GoogleCodeExchange = { handoff_id: string; authorization_code: string; code_verifier: string };
/** The existing native OAuth return owner is shared by login and deletion step-up. */
export async function nativeGoogleAuthorization(begin: (body: { code_challenge: string; state: string; redirect_uri: string }) => Promise<unknown>, status?: (message: string) => void, signal?: AbortSignal): Promise<{ body: GoogleCodeExchange; expiresAt: number }> {
  const state = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32))), verifier = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  let retired = false, failure: unknown;
  try {
    signal?.throwIfAborted();
    const callback = await controlDesktopCentralLogin("start", state);
    signal?.throwIfAborted();
    if (callback.result.status !== "pending") throw new Error("Google 로그인을 준비하지 못했어요.");
    const started = parseCentralGoogleHandoff(await begin({ code_challenge: challenge, state, redirect_uri: callback.redirect_uri }));
    const url = new URL(started.authorization_url);
    if (started.state !== state || url.searchParams.get("redirect_uri") !== callback.redirect_uri || url.searchParams.get("code_challenge") !== challenge) throw new Error("로그인 요청이 현재 앱과 일치하지 않아요.");
    signal?.throwIfAborted(); status?.("시스템 브라우저에서 Google 계정을 선택해 주세요.");
    await openDesktopCentralGoogleLogin(started.authorization_url);
    const expiresAt = Math.min(started.expires_at, callback.result.expires_at);
    while (Date.now() / 1000 < expiresAt) {
      await new Promise<void>((resolve, reject) => {
        const abort = () => { window.clearTimeout(timer); reject(signal?.reason ?? new DOMException("취소했어요.", "AbortError")); };
        const timer = window.setTimeout(() => { signal?.removeEventListener("abort", abort); resolve(); }, 1500);
        signal?.addEventListener("abort", abort, { once: true });
        if (signal?.aborted) abort();
      });
      signal?.throwIfAborted();
      const { result } = await controlDesktopCentralLogin("poll", state);
      signal?.throwIfAborted();
      if (result.status === "pending") continue;
      if (result.status !== "complete") throw new Error("Google 로그인이 취소됐어요.");
      retired = true; await controlDesktopCentralLogin("cancel", state); signal?.throwIfAborted();
      return { body: { handoff_id: started.handoff_id, code_verifier: verifier, authorization_code: result.authorization_code }, expiresAt };
    }
    throw new Error("Google 로그인 시간이 만료됐어요.");
  } catch (reason) { failure = reason; throw reason; }
  finally {
    if (!retired) {
      try { await controlDesktopCentralLogin("cancel", state); }
      catch (cleanup) {
        if (!failure) throw cleanup;
        const describe = (reason: unknown) => reason instanceof Error ? reason.message : "원인을 확인하지 못했어요.";
        throw new AggregateError([failure, cleanup], `${describe(failure)}\n로그인 정리 실패: ${describe(cleanup)}`);
      }
    }
  }
}
