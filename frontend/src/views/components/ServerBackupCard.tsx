import {useEffect,useRef,useState} from "react";
import {isDesktopWebview,requestDesktopHostDeviceInfo,requestDesktopServerBackup} from "../../lib/desktopBridge";
import {bootstrapCentral,loadCentralSession,type CentralServer} from "../../lib/central/identity";
import {strictRecord} from "../../lib/strictJsonContract";

type Progress={operation_id:string;phase:string;snapshot_at:string|null;bytes:number|null};
const phases:Record<string,string>={snapshot:"서버 내용을 준비하고 있어요",encoding:"백업 파일을 만들고 있어요",verifying:"파일을 확인하고 있어요",publishing:"백업 파일을 저장하고 있어요",complete:"백업 파일을 저장했어요",failed:"백업 파일을 만들지 못했어요. 기존 파일은 덮어쓰지 않았어요.",cancelled:"백업을 취소했어요"};
function parse(value:unknown):Progress {
  const row=strictRecord(value,"백업 진행 상태");
  if(typeof row.operation_id!=="string" || !/^[0-9a-f-]{36}$/.test(row.operation_id) || typeof row.phase!=="string" || !Object.hasOwn(phases,row.phase) ||
    (row.snapshot_at!==null&&typeof row.snapshot_at!=="string") || (row.bytes!==null&&(!Number.isSafeInteger(row.bytes)||Number(row.bytes)<0)))throw new Error("백업 진행 상태를 확인하지 못했어요.");
  return row as unknown as Progress;
}
export default function ServerBackupCard({disabled}:{disabled:boolean}) {
  const [owner,setOwner]=useState<CentralServer|null>(null),[checked,setChecked]=useState(false);
  const [encrypted,setEncrypted]=useState(true),[passphrase,setPassphrase]=useState(""),[confirm,setConfirm]=useState("");
  const [visible,setVisible]=useState(false),[progress,setProgress]=useState<Progress|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const operation=useRef(false);
  useEffect(()=>{let live=true;void (async()=>{try{
    const central=await bootstrapCentral();
    const local=isDesktopWebview()?await requestDesktopHostDeviceInfo():null;
    const owned=central?.servers.find(server=>server.relation==="owner"&&(!local||server.server_id===local.server_id));
    if(live)setOwner(owned||null);
  }catch{/* The server rechecks current ownership before export. */}finally{if(live)setChecked(true);}})();return()=>{live=false;};},[]);
  const running=Boolean(progress&&!['complete','failed','cancelled'].includes(progress.phase));
  useEffect(()=>{
    if(!running||!progress)return;
    let stopped=false;let timer:ReturnType<typeof setTimeout>|undefined;
    const poll=async()=>{try{const value=parse(await requestDesktopServerBackup({action:"status",operation_id:progress.operation_id}));if(!stopped)setProgress(value);
      if(!stopped&&!['complete','failed','cancelled'].includes(value.phase))timer=setTimeout(()=>void poll(),1000);
    }catch(reason){if(!stopped)setError(reason instanceof Error?reason.message:String(reason));}};
    timer=setTimeout(()=>void poll(),1000);
    return()=>{stopped=true;clearTimeout(timer);};
  },[running,progress?.operation_id]);
  async function start(){
    if(operation.current||!owner)return;operation.current=true;setBusy(true);setError("");
    try{const session=loadCentralSession();if(!session)throw new Error("소유 계정으로 로그인해 주세요.");
      const result=await requestDesktopServerBackup({action:"start",owner_person_id:session.person.person_id,server_name:owner.alias,passphrase:encrypted?passphrase:null});
      if(result&&typeof result==="object"&&"cancelled"in result&&result.cancelled===true)return;
      setProgress(parse(result));
    }catch(reason){setError(reason instanceof Error?reason.message:String(reason));}
    finally{operation.current=false;setBusy(false);setPassphrase("");setConfirm("");}
  }
  if(!checked||!owner)return null;
  return <section className="dc-guest-recovery-settings" aria-label="서버 백업">
    <h4>서버 백업 파일 내보내기</h4>
    {!isDesktopWebview()?<p>서버를 실행하는 기기의 앱에서 백업 파일을 저장해 주세요.</p>:<>
      <p>{owner.alias}의 방, 대화, 첨부파일, 멤버와 역할을 저장해요. 실행 중인 서버는 계속 사용할 수 있어요.</p>
      <label><input type="checkbox" checked={encrypted} disabled={disabled||busy||running} onChange={event=>setEncrypted(event.target.checked)}/> 비밀번호로 보호하기 (권장)</label>
      {encrypted?<>
        <label>비밀번호<input type={visible?"text":"password"} autoComplete="new-password" value={passphrase} disabled={busy||running} onChange={event=>setPassphrase(event.target.value)}/></label>
        <label>비밀번호 확인<input type={visible?"text":"password"} autoComplete="new-password" value={confirm} disabled={busy||running} onChange={event=>setConfirm(event.target.value)}/></label>
        <label><input type="checkbox" checked={visible} onChange={event=>setVisible(event.target.checked)}/>비밀번호 표시</label>
        <p>비밀번호를 잊으면 백업을 열 수 없어요. 앱에는 저장하지 않아요.</p>
      </>:<p>이 파일을 가진 사람은 대화와 첨부파일을 읽을 수 있어요.</p>}
      <button className="ops-button" type="button" disabled={disabled||busy||running||(encrypted&&(!passphrase||passphrase!==confirm))} onClick={()=>void start()}>저장 위치 선택</button>
      {running&&<button type="button" className="ops-button" onClick={()=>void requestDesktopServerBackup({action:"cancel",operation_id:progress?.operation_id}).then(value=>setProgress(parse(value))).catch(reason=>setError(String(reason)))}>백업 취소</button>}
      {progress&&<p role="status">{phases[progress.phase]}</p>}
      {progress?.phase==="complete"&&<p>{new Date(progress.snapshot_at!).toLocaleString()} · {progress.bytes?.toLocaleString()}바이트<br/>이 시각 이후의 변경은 들어 있지 않아요. 동기화 폴더를 골랐다면 업로드 완료도 확인해 주세요.</p>}
      {error&&<p role="alert">{error}</p>}
    </>}
  </section>;
}
