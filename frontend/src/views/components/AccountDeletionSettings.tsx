import { AccountDeletionContext } from "./AccountDeletionContext";
import { useContext, useEffect, useEffectEvent, useRef, useState } from "react";
import { X } from "lucide-react";
import { AccountDeletion, checkDeletionReceipt, clearExpiredDeletionReceipt, loadDeletionReceipt } from "../../lib/central/accountDeletion";
import { confirmDeletionGoogle } from "../../lib/central/accountDeletionGoogle";
import { CENTRAL_SESSION_CLEARED_EVENT, CENTRAL_SESSION_CHANGED_EVENT, loadCentralSession } from "../../lib/central/identity";
import LocalAccountDataChoice from "./LocalAccountDataChoice";

export default function AccountDeletionSettings({ disabled }: { disabled: boolean }) {
  const open = useContext(AccountDeletionContext);
  return <button type="button" className="ops-button" disabled={disabled} onClick={() => {
    if (!open) throw new Error("계정 탈퇴 화면을 열지 못했어요.");
    open();
  }}>계정 탈퇴</button>;
}
export function AccountDeletionFlow({ disabled, onClose }: { disabled: boolean; onClose?: () => void }) {
  const operation = useRef<AccountDeletion | null>(null);
  const cancel = useRef(new AbortController());
  const generation = useRef(0);
  const [session, setSession] = useState(loadCentralSession);
  const [busy, setBusy] = useState(false), [checking, setChecking] = useState(false);
  const busyRef = useRef(false), finishing = useRef(false);
  const activeToken = useRef<string | undefined>(undefined);
  const [code, setCode] = useState("");
  const [retainedReceipt, setRetainedReceipt] = useState(() => { try { return loadDeletionReceipt(); } catch { return null; } });
  const [error, setError] = useState(retainedReceipt && (!session || retainedReceipt.person_id === session.person.person_id) ? "탈퇴 결과를 다시 확인해 주세요." : "");
  const [deleted, setDeleted] = useState(false);
  const [, repaint] = useState(0);
  const update = () => repaint(value => value + 1);
  const sessionChanged = useEffectEvent(() => {
      const current = loadCentralSession();
      if ((operation.current && current?.token !== operation.current.session.token || busyRef.current && current?.token !== activeToken.current) && !(finishing.current && !current)) {
        operation.current?.cancel(); cancel.current.abort(); setCode("");
        setError("로그인 계정이 바뀌었어요. 탈퇴할 계정으로 다시 확인해 주세요.");
      }
      const displayedPerson = operation.current?.session.person.person_id ?? retainedReceipt?.person_id;
      if (current && displayedPerson && current.person.person_id !== displayedPerson) {
        generation.current++;
        operation.current?.cancel(); operation.current = null;
        setRetainedReceipt(null); setDeleted(false); setCode(""); setError("");
      }
      setSession(current);
  });
  useEffect(() => {
    const changed = () => sessionChanged();
    window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed);
    window.addEventListener("storage", changed);
    window.addEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed);
    return () => {
      generation.current++;
      window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed);
      window.removeEventListener("storage", changed);
      window.removeEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed);
      operation.current?.cancel(); cancel.current.abort();
    };
  }, []);
  async function start() {
    if (busyRef.current || disabled) return;
    const startedGeneration = generation.current;
    const ownsUi = () => generation.current === startedGeneration;
    busyRef.current = true; setBusy(true); setError("");
    activeToken.current = loadCentralSession()?.token;
    cancel.current.abort(); cancel.current = new AbortController();
    try {
      let previous = operation.current;
      let receipt = loadDeletionReceipt();
      if (receipt && receipt.expires_at <= Date.now() / 1000) {
        clearExpiredDeletionReceipt(receipt); setRetainedReceipt(null);
        if (previous?.requestId === receipt.request_id) { previous.cancel(); previous = null; operation.current = null; }
        receipt = null;
      }
      const current = loadCentralSession();
      if (receipt && (current ? receipt.person_id === current.person.person_id : receipt.request_id === previous?.requestId || receipt.request_id === retainedReceipt?.request_id)) {
        finishing.current = true; setChecking(true);
        if (await checkDeletionReceipt(AbortSignal.any([cancel.current.signal, AbortSignal.timeout(15_000)])) !== "account_deleted") throw new Error("탈퇴 결과를 아직 확인하지 못했어요. 잠시 후 다시 시도해 주세요.");
        if (ownsUi()) setDeleted(true); return;
      }
      if (!current || current.token !== session?.token) throw new Error("탈퇴할 계정으로 다시 로그인해 주세요.");
      const next = previous?.canRetryAuthenticated(current) ? previous.retryAuthenticated(current) : previous?.session.token === current.token ? previous : new AccountDeletion(current);
      operation.current = next;
      setChecking(true); update();
      // Google's popup opens synchronously from this click, before any inventory await.
      if (current.person.identity_kind === "guest") await next.confirmGuest(code);
      else await confirmDeletionGoogle(next, cancel.current.signal);
      if (!ownsUi()) return;
      setCode(""); setChecking(false);
      await next.inventory(); if (!ownsUi()) return; update();
      await next.removeReachable(update); if (!ownsUi()) return;
      finishing.current = true;
      await next.disable(); if (ownsUi()) setDeleted(true);
    } catch (reason) {
      if (!ownsUi()) return;
      try { setRetainedReceipt(loadDeletionReceipt()); } catch { /* The original storage/parse failure stays visible below. */ }
      const message = reason instanceof Error ? reason.message : "";
      setError(/[가-힣]/.test(message) ? message : "완료를 확인하지 못했어요. 다시 시도해 주세요.");
    } finally { finishing.current = false; busyRef.current = false; setBusy(false); setChecking(false); update(); }
  }
  function close() { generation.current++; operation.current?.cancel(); cancel.current.abort(); setCode(""); onClose?.(); }
  const op = operation.current;
  const localFollowup = op ? op.ownStopped ? "local_stopped" : op.hasOwnServers ? "server_app" : "none" : retainedReceipt?.local_followup;
  const skipped = op?.progress?.filter(item => item.state === "skipped").map(item => ({ name: item.server.name, reason: item.reason, key: `${item.server.server_id}:${item.server.registration_epoch}` }))
    ?? retainedReceipt?.results.filter(item => item.state === "skipped").map(item => ({ name: item.name, reason: item.reason, key: `${item.server_id}:${item.registration_epoch}` })) ?? [];
  const receiptOnly = retainedReceipt && retainedReceipt.expires_at > Date.now() / 1000 && (!session || retainedReceipt.person_id === session.person.person_id);
  return <section aria-label="계정 탈퇴" className="dc-account-deletion-flow" aria-busy={busy}>
    <header><h4>{deleted ? "탈퇴했어요" : "계정 탈퇴"}</h4>
      {onClose && <button type="button" className="dc-modal-close" aria-label="계정 탈퇴 닫기" onClick={close}><X size={20} /></button>}
    </header>
    {!deleted && <>
      <ul className="dc-account-deletion-notice">
        <li>모든 기기에서 로그아웃되고, 이 계정으로 다시 들어올 수 없어요.</li>
        <li>지금 연결되는 서버에서는 바로 나가요. 내가 쓴 메시지는 '탈퇴한 사용자'로 남아요.</li>
        <li>꺼져 있는 서버에는 이름과 사진이 남을 수 있어요.</li>
        <li>되돌릴 수 없어요.</li>
      </ul>
      {session && <div className="dc-account-deletion-account">
        <span className="dc-member-avatar">{session.person.avatar_url ? <img src={session.person.avatar_url} alt="" referrerPolicy="no-referrer" /> : Array.from(session.person.display_name).slice(0, 2).join("")}</span>
        <div><strong>{session.person.display_name}</strong>{session.person.identity_kind === "guest" && <p className="text-text-muted">게스트 계정</p>}</div>
      </div>}
      {op?.progress && <ul className="dc-account-deletion-servers" aria-label="서버별 탈퇴 정리 결과">{op.progress.map(item => <li key={`${item.server.server_id}:${item.server.registration_epoch}`}>
        <span>{item.server.name}{item.state === "skipped" && item.reason && <small className="text-text-muted block">{item.reason}</small>}</span><span className="dc-member-status-chip" data-state={item.state === "removed" ? "active" : item.state === "skipped" ? "attention" : "idle"}>{({ waiting: "대기", working: "나가는 중", removed: "완료", skipped: "건너뜀" })[item.state]}</span>
      </li>)}</ul>}
      {session?.person.identity_kind === "guest" && <label className="dc-account-deletion-code">복구 코드<input className="ops-input" type="password" autoComplete="off" value={code} onChange={e => setCode(e.currentTarget.value)} disabled={busy} /></label>}
      <footer><button type="button" className="dc-join-cancel" onClick={close}>취소</button>
        <button type="button" className="dc-member-session-button" data-variant="danger" disabled={disabled || busy || (!session && !receiptOnly) || (!receiptOnly && session?.person.identity_kind === "guest" && !code.trim())} onClick={() => void start()}>
          {busy ? checking ? "확인하는 중…" : "탈퇴하는 중…" : error ? "다시 시도" : session?.person.identity_kind === "guest" ? "복구 코드로 확인하고 탈퇴" : "Google로 확인하고 탈퇴"}
        </button>
      </footer>
    </>}
    {deleted && <>
      {skipped.length > 0 && <div aria-label="건너뛴 서버"><p>이 서버에는 이름과 사진이 남을 수 있어요.</p><ul className="dc-account-deletion-servers">{skipped.map(item => <li key={item.key}><span>{item.name}{item.reason && <small className="text-text-muted block">{item.reason}</small>}</span><span className="dc-member-status-chip" data-state="attention">건너뜀</span></li>)}</ul></div>}
      {localFollowup === "local_stopped" && <LocalAccountDataChoice disabled={disabled || busy} />}
      {localFollowup === "server_app" && <p>방 데이터를 지우려면 서버 컴퓨터의 앱에서 선택해 주세요. 그대로 두어도 돼요.</p>}
      <footer><button type="button" className="ops-cta min-h-11 px-4" onClick={close}>확인</button></footer>
    </>}
    {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
  </section>;
}
