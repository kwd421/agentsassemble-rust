import { AccountDeletionContext } from "./AccountDeletionContext";
import { useContext, useEffect, useRef, useState } from "react";
import { AccountDeletion, checkDeletionReceipt, loadDeletionReceipt } from "../../lib/central/accountDeletion";
import { confirmDeletionGoogle } from "../../lib/central/accountDeletionGoogle";
import { CENTRAL_SESSION_CLEARED_EVENT, CENTRAL_SESSION_CHANGED_EVENT, loadCentralSession } from "../../lib/central/identity";
import LocalAccountDataChoice from "./LocalAccountDataChoice";

export default function AccountDeletionSettings({ disabled }: { disabled: boolean }) {
  const open = useContext(AccountDeletionContext);
  return <button type="button" className="ops-button" disabled={disabled} onClick={() => {
    if (!open) throw new Error("계정 탈퇴 화면의 수명 담당을 확인하지 못했어요.");
    open();
  }}>계정 탈퇴</button>;
}
export function AccountDeletionFlow({ disabled, onClose, initial }: { disabled: boolean; onClose?: () => void; initial?: { operation: AccountDeletion | null; error: string } | null }) {
  const [returned] = useState(() => initial ?? { operation: null, error: "" });
  const operation = useRef<AccountDeletion | null>(returned.operation);
  const cancel = useRef(new AbortController());
  const [session, setSession] = useState(loadCentralSession);
  const [open, setOpen] = useState(Boolean(returned.operation || returned.error));
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const [confirmed, setConfirmed] = useState(returned.operation?.hasFreshProof ?? false);
  const [typed, setTyped] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState(returned.error);
  const [notice, setNotice] = useState("");
  const [deleted, setDeleted] = useState(false);
  const [ready, setReady] = useState(false);
  const [cancelled, setCancelled] = useState(false);
  const [, repaint] = useState(0);
  const update = () => repaint(value => value + 1);
  useEffect(() => {
    const changed = () => {
      const current = loadCentralSession();
      if (operation.current && current?.token !== operation.current.session.token) { operation.current.cancel(); cancel.current.abort(); setCancelled(true); setConfirmed(false); setReady(false); setCode(""); setNotice("로그인 계정이 바뀌어 이전 작업을 취소했어요. 이미 완료한 정리는 유지돼요."); }
      setSession(current);
    };
    window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed);
    window.addEventListener("storage", changed);
    window.addEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed);
    return () => { window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed); window.removeEventListener("storage", changed); window.removeEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed); };
  }, []);
  async function run(action: () => Promise<void>) {
    if (busyRef.current || disabled) return;
    busyRef.current = true; setBusy(true); setError("");
    try { await action(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "완료를 확인하지 못했어요. 다시 확인해 주세요."); }
    finally { busyRef.current = false; setBusy(false); update(); }
  }
  async function begin() {
    const current = loadCentralSession();
    if (!current) throw new Error("탈퇴할 계정으로 로그인해 주세요.");
    const previous = operation.current?.session.person.person_id === current.person.person_id ? operation.current.progress : null;
    operation.current?.cancel(); cancel.current.abort(); cancel.current = new AbortController();
    const next = new AccountDeletion(current); next.progress = previous ?? null; operation.current = next;
    setSession(current); setCancelled(false); setConfirmed(false); setReady(false); setTyped(""); setOpen(true); setNotice("");
    await next.inventory(); update();
  }
  const initialized = useRef(false);
  useEffect(() => {
    if (onClose && !initialized.current && !returned.operation && !returned.error && loadCentralSession()) {
      initialized.current = true; void run(begin);
    }
  }, [onClose, returned]);
  const op = operation.current;
  const results = op?.progress;
  let receipt: ReturnType<typeof loadDeletionReceipt> = null;
  try { receipt = loadDeletionReceipt(); } catch { receipt = null; }
  return <section aria-label="계정 탈퇴" className="dc-guest-recovery-settings" style={{ marginTop: 24 }} aria-busy={busy}>
    <h4>계정 탈퇴</h4>
    {onClose && <button className="ops-button" disabled={busy} onClick={() => { if (!deleted) { op?.cancel(); cancel.current.abort(); setCancelled(true); setConfirmed(false); setReady(false); setCode(""); } onClose(); }}>닫기</button>}
    {!open && session && <button className="ops-button" disabled={disabled || busy} onClick={() => void run(begin)}>계정 탈퇴</button>}
    {open && !deleted && <>
      <p>계정을 탈퇴하면 되돌릴 수 없어요. 모든 기기에서 로그아웃되고, 이 계정으로 어느 서버에도 다시 들어갈 수 없어요. 로그인 계정과 연결 관계를 삭제해요. 먼저 연결 가능한 서버에서 참가를 끝내고 이름과 사진을 지워요. 메시지와 첨부파일은 남고 작성자는 ‘탈퇴한 사용자’로 표시돼요.</p>
      <p>연결할 수 없거나 지원하지 않는 서버에는 이름·사진·기존 접속이 남을 수 있어요. 아래 결과를 확인한 뒤 중앙 계정 탈퇴를 선택해 주세요.</p>
      {session && <p>탈퇴할 계정: <strong>{session.person.display_name}</strong></p>}
      {(!op?.progress || !op.inventoryReady) && session && <button className="ops-button" disabled={disabled || busy} onClick={() => void run(begin)}>서버 목록 다시 확인</button>}
      {op?.progress && session && <>
        {session.person.identity_kind === "guest" ? <label>이 게스트 계정의 복구 코드
          <input type="password" autoComplete="off" value={code} onChange={e => setCode(e.currentTarget.value)} disabled={busy} />
        </label> : <p>처음 선택한 Google 계정으로 최근 5분 안에 다시 인증해야 해요.</p>}
        <button className="ops-button" disabled={disabled || busy || cancelled || (session.person.identity_kind === "guest" && !code.trim())} onClick={() => void run(async () => {
          setConfirmed(false);
          if (session.person.identity_kind === "guest") { const value = code; setCode(""); await op.confirmGuest(value); }
          else await confirmDeletionGoogle(op, cancel.current.signal);
          setConfirmed(op.hasFreshProof);
        })}>{confirmed ? "계정 다시 확인" : "탈퇴할 계정 확인"}</button>
        <label>계속하려면 ‘탈퇴’를 입력하세요.
          <input value={typed} autoComplete="off" onChange={e => setTyped(e.currentTarget.value)} disabled={busy} />
        </label>
        <button className="ops-button" disabled={disabled || busy || cancelled || !confirmed || !op.inventoryReady || typed !== "탈퇴"} onClick={() => void run(async () => { setReady(false); await op.removeReachable(typed, update); setReady(true); })}>서버별 참가 종료·익명화</button>
        {ready && <button className="ops-button" disabled={disabled || busy || cancelled || !confirmed || !op.inventoryReady || typed !== "탈퇴"} onClick={() => void run(async () => { await op.disable(typed); setDeleted(true); setNotice("요청한 계정의 중앙 탈퇴를 완료했어요."); })}>결과를 확인했고 중앙 계정 탈퇴</button>}
      </>}
      <button className="ops-button" disabled={busy} onClick={() => { op?.cancel(); cancel.current.abort(); setCancelled(true); setConfirmed(false); setReady(false); setCode(""); setNotice("취소했어요. 이미 완료한 서버 정리는 유지돼요."); }}>취소</button>
      <button className="ops-button" disabled={disabled || busy || !session} onClick={() => void run(begin)}>처음부터 다시 확인</button>
    </>}
    {op?.ownStopped && <p>이 컴퓨터의 서버 외부 접속과 탈퇴 계정의 연결·동반 AI를 중지했어요. 독립적인 로컬 운영자와 서버 데이터는 유지돼요.</p>}
    {results && <ul aria-label="서버별 탈퇴 정리 결과">{results.map(item => <li key={`${item.server.server_id}:${item.server.registration_epoch}`}>
      <strong>{item.server.name}</strong>{item.server.user_hidden ? " (숨긴 서버)" : ""}: {({ waiting: "대기", working: "정리 중", removed: "참가 종료·익명화 완료", skipped: "건너뜀" })[item.state]}{item.reason ? ` — ${item.reason}` : ""}
    </li>)}</ul>}
    {results?.some(item => item.state === "skipped") && <div aria-label="건너뛴 서버"><h5>건너뛴 서버</h5><ul>{results.filter(item => item.state === "skipped").map(item => <li key={item.server.server_id}>{item.server.name}: {item.reason}. 이 서버의 정리는 확인하지 못했어요.</li>)}</ul></div>}
    {!results && receipt?.results.length ? <ul aria-label="이 기기의 서버 정리 기록">{receipt.results.map(item => <li key={`${item.server_id}:${item.registration_epoch}`}>{item.name}: {item.state === "removed" ? "참가 종료·익명화 완료" : `건너뜀 — ${item.reason}`}</li>)}</ul> : null}
    {receipt && <button className="ops-button" disabled={disabled || busy} onClick={() => void run(async () => {
      const result = await checkDeletionReceipt();
      if (result === "account_deleted") { setDeleted(true); setNotice("요청한 계정의 중앙 탈퇴를 완료했어요."); }
      else setNotice("탈퇴 결과를 확인하지 못했어요. 계정을 자동으로 만들거나 다시 탈퇴하지 않아요.");
    })}>탈퇴 결과 확인</button>}
    {deleted && op?.ownStopped && <LocalAccountDataChoice disabled={disabled || busy} />}
    {deleted && op?.hasOwnServers && !op.ownStopped && <p>서버 컴퓨터에서 앱을 열어 이 컴퓨터의 방 데이터 유지·삭제를 따로 선택해 주세요. 이 웹 페이지에서는 방 데이터를 삭제하지 않았어요.</p>}
    {notice && <p role="status">{notice}</p>}{error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
  </section>;
}
