import { useEffect, useState } from "react";
import { Trash2 } from "../../ui/icons";
import type { RoomAgentSession } from "../../api";
import { deleteAgentMemory, fetchAgentMemory, type AgentMemoryPiece } from "../../api/agentMemory";
import { userFacingError } from "../../lib/apiErrors";
import "./AgentMemorySettings.css";

/** Only API agents on the built-in harness keep memory pieces. */
export function keepsMemory(session: RoomAgentSession): boolean {
  return session.runtime_kind === "api" && (session.execution_harness || "builtin") === "builtin";
}

/**
 * The memory tab: what this agent summarized from conversation older than the
 * 50 messages it sees, with a two-press delete like the card library.
 */
export default function AgentMemorySettings({ session, serverId }: { session: RoomAgentSession; serverId?: string }) {
  const [open, setOpen] = useState(false);
  const [pieces, setPieces] = useState<AgentMemoryPiece[] | null>(null);
  const [status, setStatus] = useState("");
  const [confirm, setConfirm] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setStatus("");
    setPieces(null);
    setConfirm(null);
    fetchAgentMemory(session.room_id, session.session_id, serverId)
      .then((loaded) => { if (active) setPieces(loaded); })
      .catch((error) => { if (active) setStatus(userFacingError(error, "기억을 불러오지 못했어요.")); });
    return () => { active = false; };
  }, [open, session.room_id, session.session_id, serverId]);

  async function remove(piece: AgentMemoryPiece) {
    if (confirm !== piece.from_seq) {
      setConfirm(piece.from_seq);
      setStatus("한 번 더 누르면 이 기억을 지워요.");
      return;
    }
    setBusy(true);
    try {
      await deleteAgentMemory(session.room_id, session.session_id, piece.from_seq, serverId);
      setPieces((current) => current?.filter((item) => item.from_seq !== piece.from_seq) ?? null);
      setStatus("기억을 지웠어요. 같은 대화는 다시 기억하지 않아요.");
    } catch (error) {
      setStatus(userFacingError(error, "기억을 지우지 못했어요."));
    } finally {
      setConfirm(null);
      setBusy(false);
    }
  }

  return (
    <details className="dc-agent-memory" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>기억</summary>
      <p className="dc-agent-memory-lead preserve-words">
        최근 50개보다 오래된 대화를 이 에이전트가 직접 요약해 둔 거예요. 고정한 메시지는 오래돼도 원문 그대로 함께 기억해요.
      </p>
      {open && !pieces && !status && <p className="dc-agent-memory-empty preserve-words">기억을 불러오는 중이에요.</p>}
      {pieces && pieces.length === 0 && (
        <p className="dc-agent-memory-empty preserve-words">아직 기억이 없어요. 대화가 길어지면 오래된 부분부터 요약해 둬요.</p>
      )}
      {pieces && pieces.length > 0 && (
        <ul className="dc-agent-memory-list" aria-label={`${session.display_name} 기억`}>
          {pieces.map((piece) => (
            <li key={piece.from_seq} className="dc-agent-memory-row">
              <div className="min-w-0 flex-1">
                <span className="dc-agent-memory-range">
                  메시지 #{piece.from_seq}–#{piece.to_seq} · {new Date(piece.created_at * 1000).toLocaleDateString("ko-KR")}
                </span>
                <p className="dc-agent-memory-summary preserve-words">{piece.summary}</p>
              </div>
              {confirm === piece.from_seq ? (
                <button type="button" className="dc-button dc-button--danger" disabled={busy} onClick={() => void remove(piece)}>
                  {busy ? "지우는 중" : "지우기"}
                </button>
              ) : (
                <button type="button" className="dc-agent-memory-delete" aria-label="이 기억 지우기" onClick={() => void remove(piece)}>
                  <Trash2 size={16} aria-hidden="true" />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {status && <p role="status" className="dc-agent-memory-status preserve-words">{status}</p>}
    </details>
  );
}
