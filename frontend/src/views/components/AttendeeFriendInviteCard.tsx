import { useState } from "react";
import type { useFriendsDirectory } from "../../app/useFriendsDirectory";
import type { AttendeeInvitePresentation } from "../../app/useManagedAiInvites";

export type AttendeeInviteControls = { invites: readonly AttendeeInvitePresentation[]; creating: boolean; create: (friendId: string) => void; copy: (key: string) => void };

export function AttendeeFriendInviteCard({ controls, disabled, directory }: { controls: AttendeeInviteControls; disabled: boolean; directory: ReturnType<typeof useFriendsDirectory> }) {
  const [selectedId, setSelectedId] = useState("");
  const friends = directory.friends.filter((friend) => friend.details.participant_type !== "human" && friend.details.participant_type !== "unknown");
  const selected = friends.find((friend) => friend.friend_id === selectedId);
  return <section className="dc-invite-card" aria-labelledby="attendee-friend-heading">
    <div className="dc-invite-card-copy"><h3 id="attendee-friend-heading">AI 친구 초대</h3>
      <p>친구의 컴퓨터에서 참가 명령을 실행해요. 초대는 한 번 사용할 수 있고 1시간 뒤 만료돼요.</p></div>
    <label style={{ display: "grid", gap: 8 }}>저장된 AI 친구
      <select className="ops-input" style={{ minHeight: 44, maxWidth: "100%", padding: "8px 12px" }} value={selectedId}
        disabled={directory.busy || controls.creating} onChange={(event) => setSelectedId(event.target.value)}>
        <option value="">친구 선택</option>
        {friends.map((friend) => <option key={friend.friend_id} value={friend.friend_id}>{friend.details.display_name} · {friend.details.provider_kind || "제공자 미설정"}</option>)}
      </select>
    </label>
    {directory.loaded && !friends.length && <p>친구 화면에서 AI 친구를 저장한 뒤 초대할 수 있어요.</p>}
    {selected && !selected.details.provider_kind && <p>친구 정보에 사용할 제공자 이름을 먼저 저장해 주세요.</p>}
    {directory.error && <div role="alert">{directory.error}<button type="button" className="ops-button" style={{ minHeight: 44 }} onClick={() => void directory.reload()} disabled={directory.busy}>다시 읽기</button></div>}
    <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={disabled || controls.creating || !selected?.details.provider_kind}
      onClick={() => { if (selected) controls.create(selected.friend_id); }}>{controls.creating ? "초대 만드는 중" : "AI 친구 초대 만들기"}</button>
    {controls.invites.map((invite) => <div key={invite.key} className="dc-invite-friend-row" style={{ flexWrap: "wrap" }}>
      <div className="dc-invite-card-copy"><strong>{invite.displayName} · {invite.provider}</strong><p>{invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "현재 사용할 수 없는 초대예요."}</p></div>
      <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={!invite.copyable} onClick={() => controls.copy(invite.key)}>참가 안내 복사</button>
    </div>)}
  </section>;
}
