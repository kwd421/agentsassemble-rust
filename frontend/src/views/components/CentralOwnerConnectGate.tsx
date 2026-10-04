import { LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { verifyCentralOwnerHost, type CentralOwnerConnect } from "../../lib/centralOwnerConnect";
import { exchangeCentralOwnerSession, fetchCentralOwnerRooms, persistCentralOwnerWorkspace, type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import { bindRoomDirectoryAuthority } from "../../lib/roomDirectoryContract";
import { centralAccountEntryUrl } from "../../lib/centralIdentity";

export default function CentralOwnerConnectGate({ connect, deviceToken, onComplete }: {
  connect: CentralOwnerConnect | CentralOwnerWorkspace; deviceToken: string; onComplete: (session: CentralOwnerWorkspace) => void;
}) {
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState("");
  const expired = connect.expiresAt * 1000 <= Date.now();
  useEffect(() => {
    if (connect.expiresAt * 1000 <= Date.now()) {
      setError("서버 접속이 만료됐어요. 내 서버 목록에서 서버를 다시 열어 주세요.");
      return;
    }
    let active = true;
    void (async () => {
      try {
        await verifyCentralOwnerHost(connect);
        const session = "grantToken" in connect ? await exchangeCentralOwnerSession(connect, deviceToken) : connect;
        const directory = await fetchCentralOwnerRooms(session, deviceToken);
        if (!active || !await bindRoomDirectoryAuthority(directory, null, window.location.origin, () => active)) return;
        persistCentralOwnerWorkspace(session);
        onComplete(session);
      } catch (reason) {
        if (active) setError(reason instanceof Error ? reason.message : "서버에 연결하지 못했어요.");
      }
    })();
    return () => { active = false; };
  }, [attempt, connect, deviceToken, onComplete]);
  return (
    <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
      <main className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6">
        <h1 className="text-2xl font-black text-text-primary">AgentsAssemble</h1>
        {error ? <>
          <p role="alert" className="text-sm text-[#ffb4b5]">{error}</p>
          {!expired && <button type="button" className="ops-button" onClick={() => { setError(""); setAttempt(value => value + 1); }}>다시 시도</button>}
          {centralAccountEntryUrl() && <a className="ops-button" href={centralAccountEntryUrl()}>내 서버 목록으로</a>}
        </> : <p role="status" className="flex items-center gap-2 text-sm text-text-muted"><LoaderCircle size={16} className="animate-spin" /> 서버에 연결 중이에요</p>}
      </main>
    </div>
  );
}
