import { useEffect, useState } from "react";
import { runMemberLoginPopup } from "../../lib/central/memberPopup";
let completion: Promise<void> | undefined;
export default function MemberLoginPopup() {
  const [status, setStatus] = useState("로그인을 준비하고 있어요.");
  useEffect(() => {
    completion ??= runMemberLoginPopup();
    void completion.then(() => setStatus("로그인을 마쳤어요. 초대 창으로 돌아가 주세요."), () => setStatus("로그인을 마치지 못했어요. 원래 초대 링크를 다시 열어 주세요."));
  }, []);
  return <main className="grid min-h-screen place-items-center bg-[#101114] p-6 text-text-primary"><p role="status">{status}</p></main>;
}
