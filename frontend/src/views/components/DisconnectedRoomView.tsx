import { useState } from "react";
import { ServerOff } from "lucide-react";
import type { RoomDockItem } from "../../lib/roomDockModel";

export default function DisconnectedRoomView({ room, reason, hostDisplayName, autoReconnectAt, onRetry }: {
  room: RoomDockItem;
  reason?: string | null;
  hostDisplayName?: string | null;
  /** Local reset time while the daily connection allowance is spent: the room reconnects by itself then, so no retry button. */
  autoReconnectAt?: string | null;
  onRetry: () => Promise<unknown>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function retry() {
    if (busy) return;
    setBusy(true); setError("");
    try { await onRetry(); }
    catch { setError("아직 연결하지 못했어요. 잠시 후 다시 시도해 주세요."); }
    finally { setBusy(false); }
  }
  const title = room.connectionState === "connecting" ? `‘${room.label}’에 연결하는 중이에요` : `‘${room.label}’에 연결할 수 없어요`;
  const description = autoReconnectAt ? `오늘 연결 한도를 다 썼어요. ${autoReconnectAt}에 자동으로 다시 연결돼요.`
    : reason ? reason
    : room.connectionState === "connecting" ? "연결되면 방이 바로 열려요."
    : room.connectionState === "central-unconfirmed" ? "로그인 서버에 연결하지 못했어요. 이 기기의 서버는 계속 쓸 수 있어요."
    : room.roomOrigin !== "remote_server" ? "이 기기의 서버 연결이 끊겼어요. 서버가 실행 중인지 확인해 주세요."
    : hostDisplayName ? `${hostDisplayName}님의 컴퓨터가 꺼져 있거나 인터넷이 끊겼을 수 있어요. 다시 켜지면 자동으로 연결돼요.`
    : "서버를 연 컴퓨터가 꺼져 있거나 인터넷이 끊겼을 수 있어요. 다시 켜지면 자동으로 연결돼요.";
  return (
    <section className="dc-disconnected-room" aria-labelledby="disconnected-room-title">
      <div className="dc-disconnected-room-icon" aria-hidden>
        <ServerOff size={30} />
      </div>
      <h1 id="disconnected-room-title">{title}</h1>
      <p className="dc-disconnected-room-description">
        {description}
      </p>
      {!autoReconnectAt && <button type="button" className="ops-button min-h-11 px-4" disabled={busy} onClick={() => void retry()}>{busy ? "연결 중…" : "다시 연결"}</button>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
