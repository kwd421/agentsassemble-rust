import { useRef, useState } from "react";
import { wipeDesktopAccountData } from "../../lib/desktopBridge";
/** Same-installation native capability and persisted terminal fence own actual reset. */
export default function LocalAccountDataChoice({ disabled }: { disabled: boolean }) {
  const [choice, setChoice] = useState("keep");
  const [busy, setBusy] = useState(false), [done, setDone] = useState(false), [error, setError] = useState("");
  const claimed = useRef(false);
  async function wipe() {
    if (disabled || claimed.current) return;
    claimed.current = true; setBusy(true); setError("");
    try { const result = await wipeDesktopAccountData(); setDone(true); setError(result.cleanup_errors.join("\n")); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "방 데이터 삭제 결과를 확인하지 못했어요."); }
    finally { claimed.current = false; setBusy(false); }
  }
  return <fieldset aria-busy={busy}><legend>이 컴퓨터의 방 데이터도 지울까요?</legend>
    <p>선택하면 이 컴퓨터의 방·메시지·첨부파일·AI 기록·설정을 지워요. 다른 서버에는 적용하지 않아요.</p>
    <p>서버 키·호스팅 제한과 방 데이터 밖의 로그인 정보·AI 프로그램·파일·설정·로그·그 밖의 파일은 유지돼요.</p>
    {!done && <><label><input type="checkbox" checked={choice === "wipe"} onChange={event => setChoice(event.currentTarget.checked ? "wipe" : "keep")} disabled={busy} />이 컴퓨터의 방 데이터도 지울까요?</label>
    {choice === "wipe" && <button className="ops-button" disabled={disabled || busy} onClick={() => void wipe()}>이 컴퓨터의 방 데이터 삭제</button>}</>}
    {done && <p role="status">이 컴퓨터의 방 데이터를 삭제했어요. 서버는 중지돼 있어요. 다시 사용하려면 앱을 직접 다시 시작하고 새 계정을 명시적으로 등록해 주세요.</p>}
    {error && <p role="alert">{error}</p>}
  </fieldset>;
}
