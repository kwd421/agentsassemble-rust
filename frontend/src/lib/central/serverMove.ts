import { requestDesktopServerRestore } from "../desktopBridge";
import { centralAccountEntryUrl, bootstrapCentral, loadCentralSession, signedRequest, signCentralNativeRequest, parseCentralGoogleHandoff, type CentralSession } from "./identity";
import { nativeGoogleAuthorization } from "./nativeGoogleAuthorization";
import { parseDeletionProof } from "./accountDeletion";
import { assertExactKeys, strictRecord } from "../strictJsonContract";

export type RestoreJournal = {
  operation_id: string;
  phase: "validating" | "cancelling" | "prepared" | "switch_unknown" | "not_committed" | "committed" | "activating" | "activated" | "ready";
  manifest: null | {server_id:string; server_name:string; snapshot_at:string; rooms:number; messages:number; assets:number};
  expected_epoch:string; new_key_fingerprint:string; committed_epoch:string;
};
export function parseRestoreJournal(value: unknown): RestoreJournal | null {
  if (value === null) return null;
  const row=strictRecord(value,"서버 복원 상태");
  assertExactKeys(row,["operation_id","phase","manifest","new_key_fingerprint","expected_epoch","committed_epoch","database_hash","key_hash","endpoint_generation"],"서버 복원 상태");
  if (typeof row.operation_id!=="string" || !/^[0-9a-f-]{36}$/.test(row.operation_id) ||
    !["validating","cancelling","prepared","switch_unknown","not_committed","committed","activating","activated","ready"].includes(String(row.phase)) ||
    ["expected_epoch","new_key_fingerprint","committed_epoch"].some(key=>typeof row[key]!=="string")) throw new Error("복원 상태를 확인하지 못했어요.");
  if(row.endpoint_generation!==null && (!Number.isSafeInteger(row.endpoint_generation)||Number(row.endpoint_generation)<1))throw new Error("복원 상태를 확인하지 못했어요.");
  if (row.manifest!==null) {
    const manifest=strictRecord(row.manifest,"백업 정보");
    if (typeof manifest.server_id!=="string" || !/^[0-9a-f-]{36}$/.test(manifest.server_id) ||
      typeof manifest.server_name!=="string" || typeof manifest.snapshot_at!=="string" ||
      ["rooms","messages","assets"].some(key=>!Number.isSafeInteger(manifest[key]) || Number(manifest[key])<0)) throw new Error("백업 정보를 확인하지 못했어요.");
  }
  return row as unknown as RestoreJournal;
}
export async function serverRestoreAvailable(): Promise<boolean> {
  const base = centralAccountEntryUrl();
  if (!base) return false;
  const response = await fetch(new URL("v1/config", base), {
    cache:"no-store", credentials:"omit", redirect:"error", referrerPolicy:"no-referrer",
  });
  if (!response.ok) throw new Error("서버 복원 지원 여부를 확인하지 못했어요.");
  return (await response.json()).server_restore_v1 === true;
}
export async function inspectServerRestore() { return parseRestoreJournal(await requestDesktopServerRestore({action:"inspect"})); }
export async function selectServerRestore(): Promise<boolean|null> {
  const result=strictRecord(await requestDesktopServerRestore({action:"select"}),"백업 파일 선택");
  if(result.cancelled===true)return null;
  if(typeof result.encrypted!=="boolean")throw new Error("백업 파일 형식을 확인하지 못했어요.");
  return result.encrypted;
}
export async function prepareServerRestore(passphrase:string|null) {
  if (!await serverRestoreAvailable()) throw new Error("현재 서버 복원을 시작할 수 없어요.");
  const result=await requestDesktopServerRestore({action:"prepare",passphrase});
  if (result && typeof result==="object" && "cancelled" in result && result.cancelled===true) return null;
  return parseRestoreJournal(result);
}
export async function cancelServerRestore(job:RestoreJournal) {
  await requestDesktopServerRestore({action:"cancel",operation_id:job.operation_id});
}
function current(session:CentralSession) {
  if (loadCentralSession()?.token!==session.token) throw new Error("로그인 계정이 바뀌었어요. 복원을 다시 확인해 주세요.");
}

/** Browser owns only its nonextractable device signature and fresh OAuth consent.
 * Native owns the exact switch body, durable unknown result, receipt and activation. */
export async function switchServerRestore(job:RestoreJournal,recoveryCode:string,status:(message:string)=>void,signal:AbortSignal,confirmedEpoch:string) {
  const session=loadCentralSession();
  if (!session || !job.manifest) throw new Error("백업을 소유한 계정으로 로그인해 주세요.");
  const central=await bootstrapCentral(signal); current(session);
  const owned=central?.servers.find(server=>server.relation==="owner" && server.server_id===job.manifest?.server_id);
  if (!owned?.registration_epoch || owned.registration_epoch!==confirmedEpoch || central?.owner_server_conflict) throw new Error("현재 계정의 서버 소유권을 확인하지 못했어요.");
  const bound=strictRecord(await requestDesktopServerRestore({action:"bind",operation_id:job.operation_id,expected_epoch:owned.registration_epoch}),"복원 확인");
  if (typeof bound.path!=="string" || typeof bound.request_id!=="string" || !/^[A-Za-z0-9_-]{43}$/.test(bound.request_id) || bound.path!==`/v1/servers/${job.manifest.server_id}/restore-proof`) throw new Error("복원 확인 요청이 일치하지 않아요.");
  const body=strictRecord(bound.body,"복원 확인"),requestId=bound.request_id,path=bound.path;
  const stepUp=async (step_up:Record<string,unknown>)=>{
    current(session);signal.throwIfAborted();
    const value=await signedRequest<unknown>(session,path,"POST",{...body,step_up},signal);
    current(session);return value;
  };
  let proof:unknown;
  if (session.person.identity_kind==="guest") {
    proof=await stepUp({kind:"guest",recovery_code:recoveryCode});
  } else {
    const {body:code}=await nativeGoogleAuthorization(async request=>{
      const value=strictRecord(await stepUp({kind:"google",flow_kind:"native",action:"start",...request}),"Google 복원 확인");
      if (value.request_id!==requestId || value.handoff_id!==`goh_${requestId}`) throw new Error("Google 복원 확인이 일치하지 않아요.");
      return parseCentralGoogleHandoff(value);
    },status,signal);
    proof=await stepUp({kind:"google",flow_kind:"native",action:"complete",authorization_code:code.authorization_code,code_verifier:code.code_verifier});
  }
  const fresh=parseDeletionProof(proof,requestId);
  current(session);signal.throwIfAborted();
  status("서버를 이 기기로 옮기는 중이에요. 창을 닫아도 다음 실행에서 결과를 확인할 수 있어요.");
  const draft=strictRecord(await requestDesktopServerRestore({action:"draft",operation_id:job.operation_id,owner_person_id:session.person.person_id,proof:fresh.proof}),"복원 요청");
  if (draft.path!==`/v1/servers/${job.manifest.server_id}/restore` || typeof draft.body!=="string") throw new Error("복원 요청이 일치하지 않아요.");
  const headers=await signCentralNativeRequest(session,draft.path as string,"POST",draft.body);
  current(session);signal.throwIfAborted();
  await requestDesktopServerRestore({action:"switch",operation_id:job.operation_id,body:draft.body,headers});
  return inspectServerRestore();
}
export async function resumeServerRestore(job:RestoreJournal) {
  if (job.phase==="committed" || job.phase==="activating") {
    await requestDesktopServerRestore({action:"activate",operation_id:job.operation_id});
  } else if (job.phase==="switch_unknown") {
    const session=loadCentralSession();
    if (!session || !job.manifest) throw new Error("서버 소유 계정으로 다시 로그인해 주세요.");
    const path=`/v1/servers/${job.manifest.server_id}/restore-operations/${job.operation_id}`;
    const draft=strictRecord(await requestDesktopServerRestore({action:"status_draft",operation_id:job.operation_id}),"복원 결과 확인");
    if(draft.path!==path || typeof draft.body!=="string")throw new Error("복원 결과 확인 요청이 일치하지 않아요.");
    const headers=await signCentralNativeRequest(session,path,"POST",draft.body);current(session);
    const result=strictRecord(await requestDesktopServerRestore({action:"status",operation_id:job.operation_id,headers}),"복원 결과");
    if (result.status==="superseded") throw new Error("이 서버는 이후에 다시 이동했어요. 현재 서버를 사용해 주세요.");
    if (result.status==="unknown") throw new Error("중앙 전환 결과가 아직 확인되지 않았어요. 같은 작업으로 다시 확인해 주세요.");
  }
  return inspectServerRestore();
}
