import { useState } from "react";
import { ServerOff } from "lucide-react";
import type { RoomDockItem } from "../../lib/roomDockModel";

export default function DisconnectedRoomView({ room, onRetry }: { room: RoomDockItem; onRetry: () => Promise<unknown> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function retry() {
    if (busy) return;
    setBusy(true); setError("");
    try { await onRetry(); }
    catch { setError("아직 연결하지 못했어요. 잠시 후 다시 시도해 주세요."); }
    finally { setBusy(false); }
  }
  return (
    <section className="dc-disconnected-room" aria-labelledby="disconnected-room-title">
      <div className="dc-disconnected-room-icon" aria-hidden>
        <ServerOff size={30} />
      </div>
      <p className="dc-disconnected-room-kicker">서버 연결 상태</p>
      <h1 id="disconnected-room-title">{room.connectionState === "connecting" ? "서버에 연결 중이에요" : "연결이 끊긴 방"}</h1>
      <p className="dc-disconnected-room-name">{room.label}</p>
      <p className="dc-disconnected-room-description">
        {room.connectionState === "connecting" ? "연결이 확인되면 방을 자동으로 열어요." : room.connectionState === "central-unconfirmed" ? "로그인 서버에 연결하지 못했어요. 이 기기의 서버는 계속 쓸 수 있어요."
          : room.roomOrigin !== "remote_server" ? "이 기기의 서버 연결이 끊겼어요. 서버가 실행 중인지 확인해 주세요."
          : "서버 연결이 끊겨 메시지와 참가자를 불러올 수 없어요."}
      </p>
      <button type="button" className="ops-button min-h-11 px-4" disabled={busy} onClick={() => void retry()}>{busy ? "연결 중…" : "다시 연결"}</button>
      {error && <p role="alert">{error}</p>}
      {room.serverOrigin && <code>{room.serverOrigin}</code>}
    </section>
  );
}
