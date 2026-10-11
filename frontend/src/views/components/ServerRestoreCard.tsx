import { useEffect,useRef,useState } from "react";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { bootstrapCentral,loadCentralSession } from "../../lib/central/identity";
import { serverRestoreAvailable,cancelServerRestore,inspectServerRestore,prepareServerRestore,selectServerRestore,resumeServerRestore,switchServerRestore,type RestoreJournal } from "../../lib/central/serverMove";

export default function ServerRestoreCard({onOpen}:{onOpen:()=>Promise<void>}) {
  const [open,setOpen]=useState(false),[available,setAvailable]=useState(false);
  const [currentServer,setCurrentServer]=useState<{name:string;epoch:string}|null>(null);
  const [job,setJob]=useState<RestoreJournal|null>(null),[passphrase,setPassphrase]=useState("");
  const [selected,setSelected]=useState(false);
  const [encrypted,setEncrypted]=useState(true),[recovery,setRecovery]=useState("");
  const [busy,setBusy]=useState(false),[status,setStatus]=useState(""),[error,setError]=useState("");
  const operation=useRef(false),abort=useRef<AbortController|null>(null);
  useEffect(()=>{let live=true;void serverRestoreAvailable().then(value=>{if(live)setAvailable(value);}).catch(()=>{if(live)setAvailable(false);});return()=>{live=false;};},[]);
  useEffect(()=>{let live=true;if(isDesktopWebview()) void inspectServerRestore().then(value=>{if(live){setJob(value);if(value)setOpen(true);}}).catch(reason=>{if(live)setError(String(reason));});return()=>{live=false;abort.current?.abort();};},[]);
  useEffect(()=>{
    let live=true;setCurrentServer(null);
    if(job?.manifest && ["prepared","switch_unknown"].includes(job.phase)) {
      void bootstrapCentral().then(central=>{const current=central?.servers.find(server=>server.relation==="owner"&&server.server_id===job.manifest?.server_id);
        if(live&&current?.registration_epoch)setCurrentServer({name:current.default_name||current.alias,epoch:current.registration_epoch});
      }).catch(reason=>{if(live)setError(reason instanceof Error?reason.message:String(reason));});
    }
    return()=>{live=false;};
  },[job?.operation_id,job?.phase]);
  async function run(action:()=>Promise<void>) {
    if(operation.current)return;operation.current=true;setBusy(true);setError("");
    try{await action();}catch(reason){setError(reason instanceof Error?reason.message:String(reason));
      try{setJob(await inspectServerRestore());}catch{/* Preserve the explicit operation error. */}}
    finally{operation.current=false;setBusy(false);setStatus("");setPassphrase("");setRecovery("");}
  }
  const ready=job?.phase==="activated" || job?.phase==="ready";
  const canCancel=job && ["validating","prepared","cancelling","not_committed"].includes(job.phase);
  const canSwitch=job && ["prepared","switch_unknown"].includes(job.phase);
  if(!job&&!available) return error?<p role="alert">{error}</p>:null;
  if(!open) return <section className="dc-settings-section"><button type="button" className="ops-button" onClick={()=>setOpen(true)}>백업 파일로 서버 가져오기</button></section>;
  return <section className="dc-settings-section" aria-label="서버 백업 복원">
    <h3>서버 백업으로 옮기기</h3>
    {!isDesktopWebview()?<p>백업 파일을 복원하려면 서버를 실행할 기기의 앱을 열어 주세요.</p>:<>
      <p>기존 서버의 방과 기록을 이 기기로 옮겨요. 이 기기에 서버 데이터가 있으면 덮어쓰지 않아요.</p>
      {!job?<>
        <button type="button" disabled={busy} onClick={()=>void run(async()=>{const needsPassword=await selectServerRestore();if(needsPassword!==null){setEncrypted(needsPassword);setSelected(true);}})}>백업 파일 선택</button>
        {selected&&encrypted&&<label>백업 암호<input type="password" autoComplete="off" value={passphrase} disabled={busy} onChange={event=>setPassphrase(event.target.value)}/></label>}
        {selected&&<button type="button" disabled={busy || (encrypted&&!passphrase)} onClick={()=>void run(async()=>{setStatus("백업을 검사하고 있어요.");setJob(await prepareServerRestore(encrypted?passphrase:null));})}>백업 검사</button>}
      </>:<>
        {job.manifest&&<p>{job.manifest.server_name} · 방 {job.manifest.rooms}개 · 기록 {job.manifest.messages}개<br/>백업 시각: {new Date(job.manifest.snapshot_at).toLocaleString()}</p>}
        {!ready&&job.manifest&&<p>백업 시점의 내용으로 복원해요. 백업 이후의 대화·가입·차단·초대 사용 등 변경은 복구되지 않을 수 있어요. 현재 서버는 새 서버 등록이 확정되면 기기로만 연결하게 돼요.</p>}
        {ready?<><p>백업 데이터와 새 서버 키를 설치했어요. 서버를 열어 연결 상태를 확인해 주세요.</p><button type="button" disabled={busy} onClick={()=>void run(onOpen)}>복원한 서버 열기</button></>:<>
          <p>{job.phase==="not_committed"?"이 작업은 중앙에서 전환되지 않았어요. 취소한 뒤 현재 서버 정보를 확인하고 다시 가져올 수 있어요.":job.phase==="prepared"?"백업 검사가 끝났어요. 소유 계정을 다시 확인하면 기존 기기의 서버 연결이 종료돼요.":"진행 중인 복원 작업이 있어요. 중앙 전환 결과를 확인한 뒤 이어서 진행해요."}</p>
          {canSwitch&&<>
            <p>현재 중앙 등록: {currentServer?.name || "소유 서버 정보를 확인해 주세요."}</p>
            {loadCentralSession()?.person.identity_kind==="guest"&&<label>계정 복구 코드<input type="password" autoComplete="off" value={recovery} disabled={busy} onChange={event=>setRecovery(event.target.value)}/></label>}
            <button type="button" disabled={busy||!currentServer} onClick={()=>void run(async()=>{abort.current=new AbortController();setJob(await switchServerRestore(job,recovery,setStatus,abort.current.signal,currentServer!.epoch));})}>소유 계정 확인 후 이 기기로 이동</button>
          </>}
          {!["prepared","not_committed"].includes(job.phase)&&<button type="button" disabled={busy} onClick={()=>void run(async()=>{setJob(await resumeServerRestore(job));})}>복원 상태 확인 및 재개</button>}
          {canCancel&&<button type="button" disabled={busy} onClick={()=>void run(async()=>{await cancelServerRestore(job);setJob(null);})}>복원 취소</button>}
        </>}
      </>}
      {status&&<p role="status">{status}</p>}{error&&<p role="alert">{error}</p>}
    </>}
  </section>;
}
