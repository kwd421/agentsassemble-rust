import { useEffect, useState } from "react";
import { Users } from "../../ui/icons";
import type { RoomAgentSession } from "../../api";
import { fetchAgentTrustedOwners, setAgentTrustedOwners } from "../../api/agentHandoffTrust";
import { userFacingError } from "../../lib/apiErrors";
import { useOwnerRouteHost, useRoomPeople } from "../../app/ownerRouteHost";
import "./AgentHandoffTrustSetting.css";

/**
 * Full access runs unasked only for the owner and the owner's own AIs. Here
 * the owner picks people whose AIs may hand this agent work too.
 */
export default function AgentHandoffTrustSetting({ session }: { session: RoomAgentSession }) {
  const serverId = useOwnerRouteHost() ?? null;
  const people = useRoomPeople();
  const [trusted, setTrusted] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");

  useEffect(() => {
    let active = true;
    setTrusted(null);
    setStatus("");
    fetchAgentTrustedOwners(session.room_id, session.session_id, serverId)
      .then((value) => { if (active) setTrusted(value); })
      .catch((error) => { if (active) setStatus(userFacingError(error, "설정을 불러오지 못했어요.")); });
    return () => { active = false; };
  }, [session.room_id, session.session_id, serverId]);

  async function toggle(personId: string) {
    if (!trusted) return;
    const next = trusted.includes(personId) ? trusted.filter((id) => id !== personId) : [...trusted, personId];
    setBusy(true);
    setStatus("");
    try {
      setTrusted(await setAgentTrustedOwners(session.room_id, session.session_id, next, serverId));
    } catch (error) {
      setStatus(userFacingError(error, "설정을 바꾸지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dc-agent-activity-setting dc-agent-trust-setting">
      <div className="dc-agent-activity-copy">
        <div>
          <Users size={15} aria-hidden />
          <span>함께 작업할 사람</span>
        </div>
        <p>켠 사람의 AI가 넘긴 일은 이 컴퓨터에서 묻지 않고 실행돼요. 믿는 사람만 켜세요.</p>
        {people.length === 0 && <p>방에 다른 사람이 없어요.</p>}
        {status && <p role="status">{status}</p>}
      </div>
      {people.map((person) => {
        const on = Boolean(trusted?.includes(person.id));
        const label = `${person.name}의 AI가 넘긴 일은 묻지 않기`;
        return (
          <div key={person.id} className="dc-agent-trust-person">
            <span className="min-w-0 flex-1 truncate">{person.name}</span>
            <button type="button" className="dc-agent-activity-toggle" role="switch" aria-label={label}
              aria-checked={on} data-on={on} disabled={trusted === null || busy} onClick={() => void toggle(person.id)}>
              <span className="dc-switch" aria-hidden="true"><i /></span>
              <span>{on ? "켜짐" : "꺼짐"}</span>
            </button>
          </div>
        );
      })}
    </div>
  );
}
