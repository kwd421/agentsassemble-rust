import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { loadHostToken, saveHostToken, type RoomEvent } from "./api";
import { isDesktopShell, requestDesktopTicket } from "./desktopBridge";
import {
  openRoomSocket,
  RoomSocketSayError,
  type RoomSocketHandle,
} from "./roomSocketClient";
import { acceptSnapshotEvents, mergeRoomEvents } from "./roomProjection";

type ConnectionStatus = "locked" | "connecting" | "online" | "offline";

function roomFromLocation(): string {
  return new URLSearchParams(window.location.search).get("room")?.trim() || "general";
}

export default function App() {
  const roomId = useMemo(roomFromLocation, []);
  const desktop = useMemo(isDesktopShell, []);
  const [hostToken, setHostToken] = useState(() => desktop ? "tauri-owned" : loadHostToken());
  const [draftToken, setDraftToken] = useState("");
  const [roomLabel, setRoomLabel] = useState(roomId);
  const [events, setEvents] = useState<RoomEvent[]>([]);
  const [status, setStatus] = useState<ConnectionStatus>(hostToken ? "connecting" : "locked");
  const [notice, setNotice] = useState(hostToken ? "방 기록을 동기화하는 중…" : "런타임 호스트 토큰을 입력하세요.");
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);
  const socketRef = useRef<RoomSocketHandle | null>(null);

  useEffect(() => {
    if (!hostToken) {
      socketRef.current = null;
      setStatus("locked");
      return;
    }
    setStatus("connecting");
    setNotice("방 기록을 동기화하는 중…");
    let desktopWebSocketBase = "";
    const socket = openRoomSocket(
      { kind: "host", meetingId: roomId },
      ["room_events"],
      {
        onOpen: () => {
          setStatus("connecting");
          setNotice("연결됨. 정본 스냅샷을 기다리는 중…");
        },
        onRoomSnapshot: (snapshot) => {
          setRoomLabel(String(snapshot.room.label || roomId));
          setEvents((current) => acceptSnapshotEvents(
            current,
            snapshot.events,
            snapshot.snapshot_mode
          ));
          setStatus("online");
          setNotice(snapshot.resume_gap ? "기록 공백을 정본으로 복구했습니다." : "동기화 완료");
          return true;
        },
        onRoomEvents: (incoming) => setEvents((current) => mergeRoomEvents(current, incoming)),
        onClose: () => {
          setStatus("offline");
          setNotice("연결이 끊겼습니다. 같은 커서로 다시 연결합니다…");
        },
        onError: (error) => {
          const detail = error instanceof RoomSocketSayError || error instanceof Error
            ? error.message
            : "WebSocket 연결 오류";
          setNotice(detail);
        },
      },
      desktop ? {
        getTicket: async () => {
          const grant = await requestDesktopTicket(roomId);
          desktopWebSocketBase = grant.websocket_base_url;
          return grant.ticket;
        },
        websocketBaseUrl: () => desktopWebSocketBase,
      } : {}
    );
    socketRef.current = socket;
    return () => {
      socket.close();
      if (socketRef.current === socket) socketRef.current = null;
    };
  }, [desktop, hostToken, roomId]);

  function unlock(event: FormEvent) {
    event.preventDefault();
    const token = draftToken.trim();
    if (!token) return;
    saveHostToken(token);
    setHostToken(token);
    setDraftToken("");
  }

  function lock() {
    saveHostToken("");
    setHostToken("");
    setEvents([]);
    setNotice("런타임 호스트 토큰을 입력하세요.");
  }

  async function send(event: FormEvent) {
    event.preventDefault();
    const content = message.trim();
    if (!content || !socketRef.current || sending) return;
    setSending(true);
    try {
      await socketRef.current.say({ message: content });
      setMessage("");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "메시지를 보내지 못했습니다.");
    } finally {
      setSending(false);
    }
  }

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">AGENTS ASSEMBLE · RUST RUNTIME</p>
          <h1>{roomLabel}</h1>
        </div>
        <div className={`status status--${status}`} data-testid="connection-status">
          <span aria-hidden="true" />{status}
        </div>
      </header>

      {!hostToken ? (
        <section className="unlock-card">
          <p className="eyebrow">LOCAL AUTHORITY</p>
          <h2>Rust 런타임 잠금 해제</h2>
          <p>서버를 시작할 때 설정한 32바이트 이상의 호스트 토큰이 필요합니다.</p>
          <form onSubmit={unlock}>
            <input
              aria-label="호스트 토큰"
              autoComplete="off"
              onChange={(event) => setDraftToken(event.target.value)}
              placeholder="AGENTSASSEMBLE_HOST_TOKEN"
              type="password"
              value={draftToken}
            />
            <button type="submit">연결</button>
          </form>
        </section>
      ) : (
        <section className="room-card">
          <div className="notice" role="status">
            <span>{notice}</span>
            {!desktop && <button className="link-button" onClick={lock} type="button">토큰 지우기</button>}
          </div>
          <ol className="timeline" data-testid="timeline">
            {events.length === 0 ? (
              <li className="empty">아직 메시지가 없습니다.</li>
            ) : events.map((roomEvent) => (
              <li key={roomEvent.id} data-seq={roomEvent.seq}>
                <div className="avatar">{String(roomEvent.display_name || "H").slice(0, 1)}</div>
                <div className="message-body">
                  <div className="message-meta">
                    <strong>{roomEvent.display_name || "Host"}</strong>
                    <span>#{roomEvent.seq}</span>
                  </div>
                  <p>{roomEvent.content || `[${roomEvent.type}]`}</p>
                </div>
              </li>
            ))}
          </ol>
          <form className="composer" onSubmit={send}>
            <textarea
              aria-label="메시지"
              disabled={status !== "online" || sending}
              onChange={(event) => setMessage(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  event.currentTarget.form?.requestSubmit();
                }
              }}
              placeholder={status === "online" ? `${roomLabel}에 메시지 보내기` : "연결을 기다리는 중…"}
              rows={2}
              value={message}
            />
            <button disabled={status !== "online" || sending || !message.trim()} type="submit">
              {sending ? "전송 중" : "보내기"}
            </button>
          </form>
        </section>
      )}
    </main>
  );
}
