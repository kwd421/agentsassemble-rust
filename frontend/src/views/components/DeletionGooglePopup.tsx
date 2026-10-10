import { useEffect, useState } from "react";
import { returnDeletionGooglePopup } from "../../lib/central/deletionGooglePopup";

export default function DeletionGooglePopup() {
  const [message, setMessage] = useState("Google 확인을 마치고 있어요.");
  useEffect(() => {
    let active = true;
    const controller = new AbortController();
    queueMicrotask(() => {
      if (!active) return;
      void returnDeletionGooglePopup(controller.signal).then(() => {
        if (active) { setMessage("확인했어요. 원래 창으로 돌아가 주세요."); window.close(); }
      }, () => { if (active) setMessage("확인하지 못했어요. 원래 창에서 다시 시도해 주세요."); });
    });
    return () => { active = false; controller.abort(); };
  }, []);
  return <main className="grid min-h-screen place-content-center gap-6 p-6"><h1>계정 탈퇴</h1><p role="status">{message}</p><button className="ops-button" onClick={() => window.close()}>확인</button></main>;
}
