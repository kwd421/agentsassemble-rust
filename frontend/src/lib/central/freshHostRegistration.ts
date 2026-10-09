import { fetchDesktopCentralRegistration, isDesktopWebview } from "../desktopBridge";
import { loadCentralSession, signedRequest } from "./identity";
import { assertExactKeys, strictRecord } from "../strictJsonContract";
import { verifyCentralRegistrationEnvelope, type HostRegistrationEnvelope } from "./registrationProof";

/** Explicit local administrator action; ordinary login/epoch updates never clear demotion. */
export async function registerFreshLocalHost(serverId: string, deviceToken: string): Promise<void> {
  const session = loadCentralSession();
  if (!session) throw new Error("새 계정으로 로그인해 주세요.");
  const live = () => { if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요. 다시 확인해 주세요."); };
  async function local(body: Record<string, unknown>) {
    live();
    const init = { method: "POST", cache: "no-store", headers: { "content-type": "application/json", ...(isDesktopWebview() ? {} : { "x-device-token": deviceToken }) }, body: JSON.stringify(body) } satisfies RequestInit;
    const native = isDesktopWebview() ? await fetchDesktopCentralRegistration(init) : null;
    const response = native?.response ?? await fetch("/api/central-directory/registration-proof", init);
    const value = await response.json();
    if (!response.ok) throw new Error(typeof value.error === "string" ? value.error : "이 컴퓨터의 새 등록을 확인하지 못했어요.");
    live(); return { value, native };
  }
  const state = strictRecord((await local({ server_id: serverId, hosting_state: "status" })).value, "이 컴퓨터의 서버 상태");
  assertExactKeys(state, ["hosting_state", "registration_epoch"], "이 컴퓨터의 서버 상태");
  if (state.hosting_state !== "account_deleted" || typeof state.registration_epoch !== "string" || !state.registration_epoch) throw new Error("이 컴퓨터의 탈퇴 중지 상태가 바뀌었어요. 다시 확인해 주세요.");
  const issued = await local({ owner_person_id: session.person.person_id, new_account_registration: true });
  const envelope = issued.native ? await verifyCentralRegistrationEnvelope(issued.value, session.person.person_id, issued.native.binding) : issued.value as HostRegistrationEnvelope;
  if (envelope.server_id !== serverId || envelope.registration_epoch !== undefined || envelope.host_registration_proof.owner_person_id !== session.person.person_id) throw new Error("새 등록 증명이 일치하지 않아요.");
  const registered = strictRecord(await signedRequest<unknown>(session, "/v1/servers", "POST", {
    server_id: envelope.server_id, label: envelope.host_name, host_os: envelope.host_os, name_revision: envelope.name_revision,
    host_public_key_jwk: envelope.host_public_key_jwk, host_registration_proof: envelope.host_registration_proof,
  }), "새 서버 등록");
  live();
  assertExactKeys(registered,["server_id","host_key_fingerprint","registration_epoch"],"새 서버 등록");
  if (registered.server_id !== serverId || registered.host_key_fingerprint !== envelope.host_key_fingerprint) throw new Error("새 서버 등록이 이 컴퓨터와 일치하지 않아요.");
  if (typeof registered.registration_epoch !== "string" || !registered.registration_epoch || registered.registration_epoch === state.registration_epoch) throw new Error("새 서버 등록 결과를 확인하지 못했어요.");
  await local({ server_id: serverId, new_owner_person_id: session.person.person_id, expected_registration_epoch: state.registration_epoch, registration_epoch: registered.registration_epoch });
}
