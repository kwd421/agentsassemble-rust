import { authDeviceBody, isCentralWebEntry, saveSession, unsignedPost, type CentralPerson, type CentralSession } from "./centralIdentity";
import { encodeBase64Url } from "./base64Url";

export async function prepareCentralWebGoogle(signal: AbortSignal) {
  if (!isCentralWebEntry()) throw new Error("중앙 계정 페이지에서 로그인해 주세요.");
  const verifier = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const challenge = encodeBase64Url(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  const started = await unsignedPost<{ handoff_id: string; client_id: string; nonce: string; expires_at: number }>(
    "/v1/auth/google/web/start", { ...await authDeviceBody(), code_challenge: challenge }, signal
  );
  if (!/^goh_[A-Za-z0-9_-]+$/.test(started.handoff_id) ||
      !/^[A-Za-z0-9._-]+\.apps\.googleusercontent\.com$/.test(started.client_id) ||
      !/^[A-Za-z0-9_-]{43}$/.test(started.nonce) || !Number.isSafeInteger(started.expires_at) ||
      started.expires_at <= Math.floor(Date.now() / 1000)) {
    throw new Error("웹 로그인 응답을 확인하지 못했습니다.");
  }
  return {
    clientId: started.client_id, nonce: started.nonce,
    async complete(credential: string) {
      if (signal.aborted) throw new Error("로그인이 취소됐습니다.");
      if (Math.floor(Date.now() / 1000) >= started.expires_at) throw new Error("로그인 시간이 만료됐습니다. 다시 시도해 주세요.");
      const result = await unsignedPost<{ person: CentralPerson; session: Omit<CentralSession, "person"> }>(
        "/v1/auth/google/web/complete", { handoff_id: started.handoff_id, code_verifier: verifier, credential }, signal
      );
      return saveSession(result);
    },
  };
}
