import { useEffect, useRef, useState } from "react";
import { fetchAgentOwnerSettings, MAX_AGENT_INSTRUCTIONS_CHARS, saveAgentOwnerSettings } from "../../api/agentInstructions";
import { userFacingError } from "../../lib/apiErrors";
import { useOwnerRouteHost } from "../../app/ownerRouteHost";
import "./AgentMemorySettings.css";

/**
 * The owner's own settings for one AI in this room, for app AIs and external
 * AIs alike: its instructions, which only the owner sees and the AI reads with
 * the room rules, and whether it wakes only when someone calls it.
 */
export default function AgentInstructionsSettings({ roomId, participantId, name }: { roomId: string; participantId: string; name: string }) {
  const serverId = useOwnerRouteHost() ?? null;
  const [open, setOpen] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [mentionOnly, setMentionOnly] = useState(false);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  // The AI this section shows now; a reply for another one is dropped.
  const current = useRef(participantId);
  current.current = participantId;

  useEffect(() => {
    if (!open) return;
    let active = true;
    setStatus("");
    setSaved(null);
    setDraft("");
    setMentionOnly(false);
    fetchAgentOwnerSettings(roomId, participantId, serverId)
      .then((loaded) => { if (active) { setSaved(loaded.instructions); setDraft(loaded.instructions); setMentionOnly(loaded.mention_only); } })
      .catch((error) => { if (active) setStatus(userFacingError(error, "지침을 불러오지 못했어요.")); });
    return () => { active = false; };
  }, [open, roomId, participantId, serverId]);

  const length = Array.from(draft.trim()).length;
  const tooLong = length > MAX_AGENT_INSTRUCTIONS_CHARS;
  async function save() {
    setBusy(true);
    setStatus("");
    try {
      const target = participantId;
      const stored = await saveAgentOwnerSettings(roomId, target, { instructions: draft }, serverId);
      if (current.current !== target) return;
      setSaved(stored.instructions);
      setDraft(stored.instructions);
      setStatus(stored.instructions ? "지침을 저장했어요. 다음 차례부터 따라요." : "지침을 지웠어요.");
    } catch (error) {
      setStatus(userFacingError(error, "지침을 저장하지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  async function toggleMentionOnly(next: boolean) {
    setBusy(true);
    setStatus("");
    try {
      const target = participantId;
      const stored = await saveAgentOwnerSettings(roomId, target, { mention_only: next }, serverId);
      if (current.current !== target) return;
      setMentionOnly(stored.mention_only);
      setStatus(stored.mention_only ? "이제 누가 부를 때만 반응해요." : "이제 방 설정대로 반응해요.");
    } catch (error) {
      setStatus(userFacingError(error, "설정을 저장하지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="dc-agent-memory" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>지침과 반응</summary>
      <p className="dc-agent-memory-lead preserve-words">
        이 방에서 {name}에게만 주는 지침이에요. 다른 사람에게는 보이지 않고, 방 규칙 다음으로 따라요.
      </p>
      {open && saved === null && !status && <p className="dc-agent-memory-empty preserve-words">지침을 불러오는 중이에요.</p>}
      {saved !== null && <>
        <label className="preserve-words" style={{ display: "flex", alignItems: "flex-start", gap: 8, marginTop: 8, fontSize: 14, cursor: "pointer" }}>
          <input type="checkbox" checked={mentionOnly} disabled={busy} style={{ marginTop: 3 }}
            onChange={(event) => void toggleMentionOnly(event.currentTarget.checked)} />
          <span>불렀을 때만 반응<br /><span style={{ color: "var(--color-text-muted)", fontSize: 12 }}>방 설정과 상관없이, 누가 @로 부르거나 답장으로 부를 때만 깨어나요.</span></span>
        </label>
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
