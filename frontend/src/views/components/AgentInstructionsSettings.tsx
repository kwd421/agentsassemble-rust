import { useEffect, useState } from "react";
import { fetchAgentInstructions, MAX_AGENT_INSTRUCTIONS_CHARS, saveAgentInstructions } from "../../api/agentInstructions";
import { userFacingError } from "../../lib/apiErrors";
import { useOwnerRouteHost } from "../../app/ownerRouteHost";
import "./AgentMemorySettings.css";

/**
 * The owner's own instructions for one AI in this room, for app AIs and
 * external AIs alike. Only the owner sees them; the AI reads them with the
 * room rules.
 */
export default function AgentInstructionsSettings({ roomId, participantId, name }: { roomId: string; participantId: string; name: string }) {
  const serverId = useOwnerRouteHost() ?? null;
  const [open, setOpen] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setStatus("");
    setSaved(null);
    fetchAgentInstructions(roomId, participantId, serverId)
      .then((loaded) => { if (active) { setSaved(loaded); setDraft(loaded); } })
      .catch((error) => { if (active) setStatus(userFacingError(error, "지침을 불러오지 못했어요.")); });
    return () => { active = false; };
  }, [open, roomId, participantId, serverId]);

  const length = Array.from(draft.trim()).length;
  const tooLong = length > MAX_AGENT_INSTRUCTIONS_CHARS;
  async function save() {
    setBusy(true);
    setStatus("");
    try {
      const stored = await saveAgentInstructions(roomId, participantId, draft, serverId);
      setSaved(stored);
      setDraft(stored);
      setStatus(stored ? "지침을 저장했어요. 다음 차례부터 따라요." : "지침을 지웠어요.");
    } catch (error) {
      setStatus(userFacingError(error, "지침을 저장하지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="dc-agent-memory" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>지침</summary>
      <p className="dc-agent-memory-lead preserve-words">
        이 방에서 {name}에게만 주는 지침이에요. 다른 사람에게는 보이지 않고, 방 규칙 다음으로 따라요.
      </p>
      {open && saved === null && !status && <p className="dc-agent-memory-empty preserve-words">지침을 불러오는 중이에요.</p>}
      {saved !== null && <>
        <textarea className="dc-input" aria-label={`${name} 지침`} value={draft} rows={5} disabled={busy}
          placeholder="예: 리뷰를 맡아요. 코드를 고치기 전에 먼저 물어봐요."
          style={{ width: "100%", marginTop: 8, padding: "8px 12px", resize: "vertical", fontSize: 14 }}
          onChange={(event) => setDraft(event.currentTarget.value)} />
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
          <span className="dc-agent-memory-status" style={{ marginTop: 0, flex: 1 }} aria-live="polite">
            {tooLong ? `${MAX_AGENT_INSTRUCTIONS_CHARS}자까지 쓸 수 있어요.` : `${length} / ${MAX_AGENT_INSTRUCTIONS_CHARS}`}
          </span>
          <button type="button" className="dc-button dc-button--md dc-button--primary"
            disabled={busy || tooLong || draft.trim() === saved} onClick={() => void save()}>
            {busy ? "저장 중" : "저장"}
          </button>
        </div>
      </>}
      {status && <p role="status" className="dc-agent-memory-status preserve-words">{status}</p>}
    </details>
  );
}
