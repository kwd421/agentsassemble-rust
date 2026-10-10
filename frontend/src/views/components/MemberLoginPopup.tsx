import { useEffect, useRef, useState } from "react";
import { finishMemberLoginPopup, registerMemberLoginPopup, runMemberLoginPopup } from "../../lib/central/memberPopup";
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

export default function MemberLoginPopup() {
  const [status, setStatus] = useState("로그인을 준비하고 있어요.");
  const [busy, setBusy] = useState(true);
  const [registration, setRegistration] = useState<GoogleRegistrationRequired | null>(null);
  const controller = useRef<AbortController | null>(null);
  useEffect(() => {
    let active = true, started = false, ended = false;
    const returning = new URL(window.location.href).searchParams.has("state");
    const abort = new AbortController(); controller.current = abort;
    const end = () => { if (ended) return; ended = true; abort.abort(); finishMemberLoginPopup(false); };
    // StrictMode's discarded effect must not start a second OAuth handoff.
    queueMicrotask(() => {
      if (!active) return;
      started = true;
      if (returning) window.addEventListener("pagehide", end);
      void runMemberLoginPopup(abort.signal).then(() => {
        if (active) { setStatus("로그인을 마쳤어요. 초대 창으로 돌아가 주세요."); setBusy(false); }
      }, error => {
        if (!active) return;
        setBusy(false);
        if (error instanceof GoogleRegistrationRequired) { setRegistration(error); setStatus(error.message); }
        else setStatus("로그인을 마치지 못했어요. 초대 창에서 Google로 다시 시도해 주세요. 초대 창을 닫았다면 원래 초대 링크를 다시 열어 주세요.");
      });
    });
    return () => { active = false; window.removeEventListener("pagehide", end); if (started) end(); else abort.abort(); };
  }, []);
  async function register() {
    if (!registration || busy || !controller.current) return;
    setBusy(true);
    try {
      await registerMemberLoginPopup(registration, controller.current.signal);
      setRegistration(null); setStatus("가입하고 로그인했어요. 초대 창으로 돌아가 주세요.");
    } catch (error) {
      const messages = [
        "새 계정을 만들었어요. Google로 다시 로그인해 주세요.",
        "확인 시간이 만료됐어요. Google 계정을 다시 확인해 주세요.",
        "로그인 계정이 바뀌었어요. Google 계정을 다시 확인해 주세요.",
        "새 계정 결과를 확인하지 못했어요.",
      ];
      setRegistration(null); setStatus(error instanceof Error && messages.includes(error.message)
        ? error.message : "가입 결과를 확인하지 못했어요. 초대 창에서 Google로 다시 시도해 주세요.");
    } finally { setBusy(false); }
  }
  function returnToInvite() {
    controller.current?.abort(); finishMemberLoginPopup(false);
    window.opener?.focus(); window.close();
  }
  return <GuestJoinProfilePanel title={registration ? "Google 계정 확인" : "Google 로그인"}
    displayName="" status={status} busy={busy} onDisplayNameChange={() => {}} onAvatarImageChange={() => {}} onJoin={returnToInvite}>
    <section className="grid gap-3" aria-label="Google 로그인">
      {registration && <button type="button" className="dc-guest-join-button" disabled={busy} onClick={() => void register()}>새로 가입</button>}
      <button type="button" className="dc-join-cancel" disabled={busy} onClick={returnToInvite}>초대 창으로 돌아가서 다시 시도</button>
    </section>
  </GuestJoinProfilePanel>;
}
