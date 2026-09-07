import { useEffect, useState } from "react";
import type { RoomAgentSession } from "../../../api";

export default function AgentIdentitySettings({ session, onSave }: {
  session: RoomAgentSession;
  onSave: (session: RoomAgentSession, settings: Record<string, string>) => void | Promise<void>;
}) {
  const [name, setName] = useState(session.display_name);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  useEffect(() => { setName(session.display_name); }, [session.display_name]);

  async function saveProfile() {
    if (busy) return;
    setBusy(true);
    setStatus("");
    try {
      await onSave(session, { display_name: name });
      setStatus("에이전트 프로필 저장됨");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "에이전트 프로필 저장 실패");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="dc-agent-runtime-settings" aria-label={`${session.display_name} 에이전트 프로필`}>
      <label>
        <span>표시 이름</span>
        <input type="text" value={name} disabled={busy}
          onChange={(event) => setName(event.currentTarget.value)}
          placeholder={session.display_name} />
      </label>
      <button type="button" className="dc-member-session-button" disabled={busy || !name.trim()}
        onClick={() => void saveProfile()}>
        프로필 저장
      </button>
      {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
    </section>
  );
}
