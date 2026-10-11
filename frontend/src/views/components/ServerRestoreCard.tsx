import { useEffect,useRef,useState } from "react";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { bootstrapCentral,loadCentralSession } from "../../lib/central/identity";
import { serverRestoreAvailable,cancelServerRestore,inspectServerRestore,prepareServerRestore,selectServerRestore,resumeServerRestore,switchServerRestore,type RestoreJournal } from "../../lib/central/serverMove";

export default function ServerRestoreCard({onOpen,liveHost=false}:{onOpen:()=>Promise<void>;liveHost?:boolean}) {
  const [open,setOpen]=useState(false),[available,setAvailable]=useState(false);
  const [currentServer,setCurrentServer]=useState<{name:string;epoch:string}|null>(null);
  const [job,setJob]=useState<RestoreJournal|null>(null),[passphrase,setPassphrase]=useState("");
  const [selected,setSelected]=useState<string|null>(null);
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
  // A finished move on the computer that now hosts the server needs no card.
  if(liveHost&&!error&&(!job||ready)) return null;
  const primary="min-h-10 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50";
  const secondary="min-h-10 rounded-md border border-white/10 px-4 text-[13px] font-bold text-text-primary hover:bg-white/5 disabled:opacity-50";
  const quiet="min-h-10 px-2 text-[13px] text-text-muted hover:text-text-primary hover:underline disabled:opacity-50";
  const field="mt-1 min-h-10 w-full rounded-md border border-white/10 bg-[#121214] px-3 text-[13px] text-text-primary outline-none focus:border-[#5865f2]";
  const label="grid text-[12px] font-bold text-text-muted";
  const pickFile=()=>void run(async()=>{const picked=await selectServerRestore();if(picked){setEncrypted(picked.encrypted);setSelected(picked.fileName||"백업 파일");}});
  if(!open) return <section className="dc-settings-section"><button type="button" className={quiet} onClick={()=>setOpen(true)}>백업 파일로 서버 옮겨오기</button></section>;
  return <section className="dc-settings-section grid gap-3" aria-label="서버 옮겨오기">
    <h3 className="text-[14px] font-black text-text-primary">서버 옮겨오기</h3>
    {!isDesktopWebview()?<p className="text-[13px] text-text-muted">서버로 쓸 컴퓨터의 앱에서 열어 주세요.</p>:<>
      {!job?<>
        {selected===null?<button type="button" className={`${secondary} justify-self-start`} disabled={busy} onClick={pickFile}>백업 파일 선택</button>:<>
          <div className="flex min-h-10 items-center gap-2 rounded-md bg-white/5 px-3">
            <span className="min-w-0 flex-1 truncate text-[13px] text-text-primary" title={selected}>{selected}</span>
            <button type="button" className={quiet} disabled={busy} onClick={pickFile}>다른 파일</button>
          </div>
          {encrypted&&<label className={label}>백업 비밀번호<input className={field} type="password" autoComplete="off" value={passphrase} disabled={busy} onChange={event=>setPassphrase(event.target.value)}/></label>}
          <button type="button" className={`${primary} justify-self-end`} disabled={busy || (encrypted&&!passphrase)} onClick={()=>void run(async()=>{setStatus("백업을 확인하고 있어요.");setJob(await prepareServerRestore(encrypted?passphrase:null));})}>백업 확인</button>
        </>}
      </>:<>
        {job.manifest&&!ready&&<div className="rounded-md bg-white/5 px-3 py-2">
          <div className="truncate text-[13px] font-bold text-text-primary">{job.manifest.server_name}</div>
          <div className="text-[12px] text-text-muted">방 {job.manifest.rooms}개 · 메시지 {job.manifest.messages}개 · {new Date(job.manifest.snapshot_at).toLocaleString()} 백업</div>
        </div>}
        {ready?<>
          <p className="text-[13px] text-text-primary">서버를 옮겼어요.</p>
          <button type="button" className={`${primary} justify-self-end`} disabled={busy} onClick={()=>void run(onOpen)}>서버 열기</button>
        </>:<>
          {job.phase==="prepared"&&<p className="text-[13px] text-text-muted">옮기면 {currentServer?`‘${currentServer.name}’은(는)`:"지금 서버는"} 더 이상 서버로 쓸 수 없어요. 백업 뒤에 생긴 대화는 옮겨지지 않아요.</p>}
          {job.phase==="not_committed"&&<p className="text-[13px] text-text-muted">옮기지 못했어요. 취소한 뒤 다시 시도해 주세요.</p>}
          {!["prepared","not_committed","validating","cancelling"].includes(job.phase)&&<p className="text-[13px] text-text-muted">옮기던 작업이 있어요.</p>}
          {canSwitch&&loadCentralSession()?.person.identity_kind==="guest"&&<label className={label}>계정 복구 코드<input className={field} type="password" autoComplete="off" value={recovery} disabled={busy} onChange={event=>setRecovery(event.target.value)}/></label>}
          <div className="flex items-center justify-end gap-2">
            {canCancel&&<button type="button" className={quiet} disabled={busy} onClick={()=>void run(async()=>{await cancelServerRestore(job);setJob(null);setSelected(null);})}>취소</button>}
            {!["prepared","not_committed"].includes(job.phase)&&<button type="button" className={canSwitch?secondary:primary} disabled={busy} onClick={()=>void run(async()=>{setJob(await resumeServerRestore(job));})}>이어서 진행</button>}
            {canSwitch&&<button type="button" className={primary} disabled={busy||!currentServer} onClick={()=>void run(async()=>{abort.current=new AbortController();setJob(await switchServerRestore(job,recovery,setStatus,abort.current.signal,currentServer!.epoch));})}>이 컴퓨터로 옮기기</button>}
          </div>
        </>}
      </>}
      {status&&<p role="status" className="text-[12px] text-text-muted">{status}</p>}{error&&<p role="alert" className="text-[12px] text-[#f38b8b]">{error}</p>}
    </>}
  </section>;
}
