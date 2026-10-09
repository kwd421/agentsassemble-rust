import { encodeBase64Url } from "../base64Url";
import { loadCentralSession, saveSession, unsignedPost, type CentralPerson, type CentralSession } from "./identity";
import { assertExactKeys, strictRecord } from "../strictJsonContract";
import type { GoogleCodeExchange } from "./nativeGoogleAuthorization";

type LoginResult = { status: "complete"; person: CentralPerson; session: Omit<CentralSession, "person"> };
export class GoogleRegistrationRequired extends Error {
  #body: GoogleCodeExchange;
  #flow: "native" | "web";
  #expiresAt: number;
  #expectedToken: string | null;
  constructor(readonly identityStatus: "deleted" | "absent", flow: "native" | "web", body: GoogleCodeExchange, expiresAt: number, expectedToken: string | null) {
    super(identityStatus === "deleted" ? "탈퇴한 Google 계정이에요. 새 계정을 만들면 이전 연결 관계를 이어받지 않아요." : "아직 가입하지 않은 Google 계정이에요. 새 계정을 만들려면 따로 선택해 주세요.");
    this.#body = body; this.#flow = flow; this.#expiresAt = expiresAt; this.#expectedToken = expectedToken;
  }
  async register(): Promise<CentralSession> {
    if (this.#expiresAt <= Date.now() / 1000) throw new Error("확인 시간이 만료됐어요. Google 계정을 다시 확인해 주세요.");
    const live = () => { if ((loadCentralSession()?.token ?? null) !== this.#expectedToken) throw new Error("로그인 계정이 바뀌었어요. Google 계정을 다시 확인해 주세요."); };
    live();
    const value = strictRecord(await unsignedPost<unknown>(`/v1/auth/google/${this.#flow}/register`, this.#body), "Google 새 가입");
    live();
    if (value.status === "registered" && value.login_required === true) throw new Error("새 계정을 만들었어요. Google로 다시 로그인해 주세요.");
    if (value.status !== "complete") throw new Error("새 계정 결과를 확인하지 못했어요.");
    assertExactKeys(value, ["status", "person", "session"], "Google 새 가입");
    return saveSession(value as LoginResult);
  }
}

export function finishGoogleVerification(value: unknown, flow: "native" | "web", body: GoogleCodeExchange, expiresAt: number, expectedToken: string | null): CentralSession {
  const r = strictRecord(value, "Google 계정 확인");
  if (r.status === "deleted" || r.status === "absent") {
    assertExactKeys(r, ["status"], "Google 계정 확인");
    throw new GoogleRegistrationRequired(r.status, flow, body, expiresAt, expectedToken);
  }
  if (r.status !== "complete") throw new Error("Google 계정 확인 결과를 확인하지 못했어요.");
  assertExactKeys(r, ["status", "person", "session"], "Google 계정 확인");
  if ((loadCentralSession()?.token ?? null) !== expectedToken) throw new Error("로그인 계정이 바뀌었어요. 다시 확인해 주세요.");
  return saveSession(r as LoginResult);
}

export async function centralSessionFingerprint(): Promise<string | null> {
  const token=loadCentralSession()?.token;
  return token ? encodeBase64Url(await crypto.subtle.digest("SHA-256",new TextEncoder().encode(token))) : null;
}
