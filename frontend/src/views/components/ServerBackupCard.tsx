import {useEffect,useRef,useState} from "react";
import {isDesktopWebview,requestDesktopHostDeviceInfo,requestDesktopServerBackup} from "../../lib/desktopBridge";
import {bootstrapCentral,loadCentralSession,type CentralServer} from "../../lib/central/identity";
import {strictRecord} from "../../lib/strictJsonContract";

type Progress={operation_id:string;phase:string;snapshot_at:string|null;bytes:number|null;failure_reason?:string|null};
const phases:Record<string,string>={snapshot:"서버 내용을 준비하고 있어요",encoding:"백업 파일을 만들고 있어요",verifying:"파일을 확인하고 있어요",publishing:"백업 파일을 저장하고 있어요",complete:"백업 파일을 저장했어요",failed:"백업 파일을 만들지 못했어요. 기존 파일은 덮어쓰지 않았어요.",cancelled:"백업을 취소했어요",interrupted:"백업 저장 완료를 확인하지 못했어요. 저장 위치를 확인한 뒤 새 백업을 만들어 주세요."};
const failureReasons:Record<string,string>={
  permission_denied:"파일 접근 권한이 없어요. 저장 폴더의 권한을 확인해 주세요.",
  destination_exists:"같은 이름의 파일이 있어요. 다른 이름으로 저장해 주세요.",
  storage_full:"저장 공간이 부족해요. 앱 데이터와 저장 폴더의 여유 공간을 확보해 주세요.",
  path_unavailable:"저장 위치를 찾지 못했어요. 연결된 폴더를 다시 선택해 주세요.",
  io:"파일을 읽거나 저장하지 못했어요. 저장 장치와 접근 권한을 확인해 주세요.",
  database_busy:"서버 데이터가 사용 중이라 백업하지 못했어요. 잠시 후 다시 시도해 주세요.",
  database:"서버 데이터 복사에 실패했어요. 다시 시도하고 문제가 계속되면 로그를 확인해 주세요.",
  invalid_data:"서버 데이터가 백업 형식 검증을 통과하지 못했어요. 앱 버전과 로그를 확인해 주세요.",
  encryption:"백업 암호화에 실패했어요. 앱을 다시 시작한 뒤 시도해 주세요.",
  verification:"생성한 백업 파일의 무결성을 확인하지 못했어요. 다시 시도해 주세요.",
  internal:"백업 작업 상태를 처리하지 못했어요. 로그를 확인하고 다시 시도해 주세요.",
};
function parse(value:unknown):Progress {
  const row=strictRecord(value,"백업 진행 상태");
  if(typeof row.operation_id!=="string" || !/^[0-9a-f-]{36}$/.test(row.operation_id) || typeof row.phase!=="string" || !Object.hasOwn(phases,row.phase) ||
    (row.snapshot_at!==null&&typeof row.snapshot_at!=="string") || (row.bytes!==null&&(!Number.isSafeInteger(row.bytes)||Number(row.bytes)<0)))throw new Error("백업 진행 상태를 확인하지 못했어요.");
  if(row.failure_reason!=null&&(typeof row.failure_reason!=="string"||!Object.hasOwn(failureReasons,row.failure_reason)))throw new Error("백업 실패 이유를 확인하지 못했어요.");
  return row as unknown as Progress;
}
export default function ServerBackupCard({disabled}:{disabled:boolean}) {
  const [owner,setOwner]=useState<CentralServer|null>(null),[checked,setChecked]=useState(false);
  const [encrypted,setEncrypted]=useState(true),[passphrase,setPassphrase]=useState(""),[confirm,setConfirm]=useState("");
  const [visible,setVisible]=useState(false),[progress,setProgress]=useState<Progress|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const operation=useRef(false);
  const [statusRetry,setStatusRetry]=useState(0);
  useEffect(()=>{let live=true;void (async()=>{try{
    const central=await bootstrapCentral();
    const local=isDesktopWebview()?await requestDesktopHostDeviceInfo():null;
    const owned=central?.servers.find(server=>server.relation==="owner"&&(!local||server.server_id===local.server_id));
    if(live)setOwner(owned||null);
    if(local){const latest=await requestDesktopServerBackup({action:"latest"});if(live&&latest!==null)setProgress(parse(latest));}
  }catch(reason){if(live)setError(reason instanceof Error?reason.message:String(reason));}finally{if(live)setChecked(true);}})();return()=>{live=false;};},[]);
  const running=Boolean(progress&&!['complete','failed','cancelled','interrupted'].includes(progress.phase));
  useEffect(()=>{
    if(!running||!progress)return;
    let stopped=false;let timer:ReturnType<typeof setTimeout>|undefined;
    const poll=async()=>{try{const value=parse(await requestDesktopServerBackup({action:"status",operation_id:progress.operation_id}));if(!stopped)setProgress(value);
      if(!stopped&&!['complete','failed','cancelled','interrupted'].includes(value.phase))timer=setTimeout(()=>void poll(),1000);
    }catch(reason){if(!stopped)setError(reason instanceof Error?reason.message:String(reason));}};
    timer=setTimeout(()=>void poll(),1000);
    return()=>{stopped=true;clearTimeout(timer);};
  },[running,progress?.operation_id,statusRetry]);
  async function refresh(){
    try{const latest=await requestDesktopServerBackup({action:"latest"});setProgress(latest===null?null:parse(latest));setError("");setStatusRetry(value=>value+1);}
    catch(reason){setError(reason instanceof Error?reason.message:String(reason));}
  }
  async function start(){
    if(operation.current||!owner)return;operation.current=true;setBusy(true);setError("");
    try{const session=loadCentralSession();if(!session)throw new Error("소유 계정으로 로그인해 주세요.");
      const result=await requestDesktopServerBackup({action:"start",owner_person_id:session.person.person_id,server_name:owner.alias,passphrase:encrypted?passphrase:null});
      if(result&&typeof result==="object"&&"cancelled"in result&&result.cancelled===true)return;
      setProgress(parse(result));
    }catch(reason){setError(reason instanceof Error?reason.message:String(reason));
      try{const latest=await requestDesktopServerBackup({action:"latest"});if(latest!==null)setProgress(parse(latest));}catch{/* Keep the original failure and expose explicit status retry. */}
    }
    finally{operation.current=false;setBusy(false);setPassphrase("");setConfirm("");}
  }
  if(!checked)return <section className="dc-guest-recovery-settings" aria-label="서버 백업" aria-busy="true"><h4>서버 백업 파일 내보내기</h4><p role="status">백업 정보를 불러오고 있어요…</p></section>;
  if(!owner)return error?<section className="dc-guest-recovery-settings" aria-label="서버 백업"><p role="alert">{error}</p></section>:null;
  return <section className="dc-guest-recovery-settings" aria-label="서버 백업">
    <h4>서버 백업 파일 내보내기</h4>
    {!isDesktopWebview()?<p>서버를 실행하는 기기의 앱에서 백업 파일을 저장해 주세요.</p>:<>
      <p>{owner.alias}의 방, 대화, 첨부파일, 멤버와 역할을 저장해요. 실행 중인 서버는 계속 사용할 수 있어요.</p>
      <label><input type="checkbox" checked={encrypted} disabled={disabled||busy||running} onChange={event=>setEncrypted(event.target.checked)}/> 비밀번호로 보호하기 (권장)</label>
      {encrypted?<>
        <label>비밀번호<input className="ops-input" type={visible?"text":"password"} autoComplete="new-password" value={passphrase} disabled={busy||running} onChange={event=>setPassphrase(event.target.value)}/></label>
        <label>비밀번호 확인<input className="ops-input" type={visible?"text":"password"} autoComplete="new-password" value={confirm} disabled={busy||running} onChange={event=>setConfirm(event.target.value)}/></label>
        <label><input type="checkbox" checked={visible} onChange={event=>setVisible(event.target.checked)}/>비밀번호 표시</label>
        <p>비밀번호를 잊으면 백업을 열 수 없어요. 앱에는 저장하지 않아요.</p>
      </>:<p>이 파일을 가진 사람은 대화와 첨부파일을 읽을 수 있어요.</p>}
      <button className="ops-button" type="button" disabled={disabled||busy||running||(encrypted&&(!passphrase||passphrase!==confirm))} onClick={()=>void start()}>저장 위치 선택</button>
      {running&&<button type="button" className="ops-button" onClick={()=>void requestDesktopServerBackup({action:"cancel",operation_id:progress?.operation_id}).then(value=>setProgress(parse(value))).catch(reason=>setError(String(reason)))}>백업 취소</button>}
      {progress&&<p role="status">{phases[progress.phase]}{progress.failure_reason&&<> {failureReasons[progress.failure_reason]}</>}</p>}
      {progress?.phase==="complete"&&<p>{new Date(progress.snapshot_at!).toLocaleString()} · {progress.bytes?.toLocaleString()}바이트<br/>이 시각 이후의 변경은 들어 있지 않아요. 동기화 폴더를 골랐다면 업로드 완료도 확인해 주세요.</p>}
      {error&&<><p role="alert">{error}</p><button type="button" className="ops-button" onClick={()=>void refresh()}>진행 상태 다시 확인</button></>}
    </>}
  </section>;
}
