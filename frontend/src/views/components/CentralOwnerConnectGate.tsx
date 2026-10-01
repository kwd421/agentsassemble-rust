import { LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";

import {
  enterCentralOwnerRoom,
  fetchCentralOwnerRooms,
  verifyCentralOwnerHost,
  type CentralOwnerConnect,
  type CentralOwnerRoom,
} from "../../lib/centralOwnerConnect";
import {
  persistRoomGuestSession,
  roomGuestSessionFromPairingPayload,
} from "../../lib/roomGuestSession";

export default function CentralOwnerConnectGate({
  connect,
  deviceToken,
  onComplete,
}: {
  connect: CentralOwnerConnect;
  deviceToken: string;
  onComplete: () => void;
}) {
  const [rooms, setRooms] = useState<CentralOwnerRoom[]>([]);
  const [busyRoom, setBusyRoom] = useState("");
  const [checking, setChecking] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        await verifyCentralOwnerHost(connect);
        const loaded = await fetchCentralOwnerRooms(connect, deviceToken);
        if (active) setRooms(loaded);
      } catch (reason) {
        if (active) {
          setError(reason instanceof Error ? reason.message : "중앙 서버를 확인하지 못했습니다.");
        }
      } finally {
        if (active) setChecking(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [connect, deviceToken]);

  async function enter(room: CentralOwnerRoom) {
    if (busyRoom) return;
    setBusyRoom(room.roomId);
    setError("");
    try {
      const payload = await enterCentralOwnerRoom(
        connect,
        deviceToken,
        room.roomId,
        room.roomUid
      );
      const session = roomGuestSessionFromPairingPayload(payload);
      if (session.meetingId !== room.roomId) {
        throw new Error("선택한 방과 발급된 방 세션이 다릅니다.");
      }
      if (session.roomUid && session.roomUid !== room.roomUid) {
        throw new Error("선택한 방의 세대가 바뀌었습니다. 다시 연결해 주세요.");
      }
      persistRoomGuestSession(session);
      onComplete();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "방을 열지 못했습니다.");
      setBusyRoom("");
    }
  }

  return (
    <div className="fixed inset-0 z-[400] grid place-items-center overflow-y-auto bg-[#101114] p-5">
      <main className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl">
        <header className="grid gap-2">
          <span className="text-[11px] font-black uppercase tracking-[0.16em] text-[#8d96ff]">
            중앙 계정 · 내 서버
          </span>
          <h1 className="text-2xl font-black text-text-primary">열 방을 선택하세요</h1>
          <p className="text-[13px] font-semibold leading-5 text-text-muted">
            서버 키와 현재 접속 주소를 확인한 뒤 이 브라우저에만 짧은 방 세션을 발급합니다.
          </p>
        </header>
        {checking && (
          <p role="status" className="flex items-center gap-2 text-[12px] font-bold text-text-muted">
            <LoaderCircle size={16} className="animate-spin" /> 서버 소유권 확인 중
          </p>
        )}
        {error && (
          <p role="alert" className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]">
            {error}
          </p>
        )}
        {!checking && !error && rooms.length === 0 && (
          <p className="rounded-md bg-[#2b2d31] p-3 text-[12px] font-bold text-text-muted">
            지금 열 수 있는 방이 없습니다.
          </p>
        )}
        <div className="grid gap-2">
          {rooms.map((room) => (
            <button
              key={room.roomUid}
              type="button"
              disabled={Boolean(busyRoom) || room.status !== "active"}
              onClick={() => void enter(room)}
              className="grid min-h-14 gap-1 rounded-md bg-[#2b2d31] px-4 py-3 text-left disabled:opacity-50"
            >
              <span className="text-[13px] font-black text-text-primary">
                {busyRoom === room.roomId ? "여는 중…" : room.label}
              </span>
              <span className="text-[11px] font-semibold text-text-muted">
                {room.topic || (room.status === "active" ? "활성 방" : "닫힌 방")}
              </span>
            </button>
          ))}
        </div>
      </main>
    </div>
  );
}
