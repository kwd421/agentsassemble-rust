import { useEffect, useState } from "react";
import { Users } from "../../ui/icons";
import type { RoomAgentSession } from "../../api";
import { fetchAgentHandoffTrust, setAgentHandoffTrust } from "../../api/agentHandoffTrust";
import { userFacingError } from "../../lib/apiErrors";

const LABEL = "다른 사람 AI가 넘긴 일도 묻지 않기";

/**
 * Full access runs unasked only for the owner and the owner's own AIs. This
 * switch lets other people's AIs in the room hand this agent work too.
 */
export default function AgentHandoffTrustSetting({ session }: { session: RoomAgentSession }) {
  const [trusted, setTrusted] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");

  useEffect(() => {
    let active = true;
    setTrusted(null);
    setStatus("");
    fetchAgentHandoffTrust(session.room_id, session.session_id)
      .then((value) => { if (active) setTrusted(value); })
      .catch((error) => { if (active) setStatus(userFacingError(error, "설정을 불러오지 못했어요.")); });
    return () => { active = false; };
  }, [session.room_id, session.session_id]);

  async function toggle() {
    if (trusted === null) return;
    setBusy(true);
    setStatus("");
    try {
      setTrusted(await setAgentHandoffTrust(session.room_id, session.session_id, !trusted));
    } catch (error) {
      setStatus(userFacingError(error, "설정을 바꾸지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dc-agent-activity-setting">
      <div className="dc-agent-activity-copy">
        <div>
          <Users size={15} aria-hidden />
          <span>{LABEL}</span>
        </div>
        <p>켜면 이 방의 다른 사람이 자기 AI를 통해 부탁한 명령도 이 컴퓨터에서 묻지 않고 실행돼요. 믿는 사람과 함께 작업할 때만 켜세요.</p>
        {status && <p role="status">{status}</p>}
      </div>
      <button type="button" className="dc-agent-activity-toggle" role="switch" aria-label={LABEL}
        aria-checked={Boolean(trusted)} data-on={Boolean(trusted)} disabled={trusted === null || busy} onClick={() => void toggle()}>
        <span className="dc-switch" aria-hidden="true"><i /></span>
        <span>{trusted ? "켜짐" : "꺼짐"}</span>
      </button>
    </div>
  );
}
