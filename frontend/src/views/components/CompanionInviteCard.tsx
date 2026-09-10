import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";

export default function CompanionInviteCard({ controls, providers }: { controls: CompanionInviteControls; providers?: NativeCliProviderAvailability[] }) {
  return <section className="dc-invite-card" style={{ margin: "20px 24px", minWidth: 0 }} aria-label="동반 AI 초대">
    <div className="dc-invite-card-copy"><h3>동반 AI 초대</h3><p>내 컴퓨터에서 실행한 AI가 함께 참가해요. 내가 방을 나가면 AI도 더 이상 대화에 참여할 수 없어요.</p></div>
    <label style={{ display: "grid", gap: 8 }}>AI 이름
      <input className="ops-input" style={{ minHeight: 44, minWidth: 0 }} maxLength={80} value={controls.displayName} disabled={controls.creating} onChange={(event) => controls.setDisplayName(event.target.value)} />
    </label>
    <label style={{ display: "grid", gap: 8 }}>제공자 이름
      {providers ? <select aria-label="제공자" className="ops-input" style={{ minHeight: 44 }} value={controls.provider} disabled={controls.creating}
        onChange={(event) => controls.setProvider(event.currentTarget.value)}><option value="">제공자 선택</option>
        {providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.display_name}</option>)}
      </select> : <input className="ops-input" style={{ minHeight: 44, minWidth: 0 }} placeholder="예: codex" value={controls.provider} disabled={controls.creating} onChange={(event) => controls.setProvider(event.target.value)} />}
    </label>
    <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={controls.creating || !controls.provider.trim() || !controls.displayName.trim()} onClick={() => void controls.create()}>{controls.creating ? "초대 만드는 중" : "동반 AI 초대 만들기"}</button>
    {controls.status && <p role="status">{controls.status}</p>}
    {controls.invites.map((invite) => <div key={invite.key} className="dc-invite-friend-row" style={{ flexWrap: "wrap" }}>
      <div className="dc-invite-card-copy"><strong>{invite.displayName} · {invite.provider}</strong><p>{invite.joined ? "방 참가가 확인됐어요. 실행 상태는 에이전트 카드에서 확인해 주세요." : invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "만료됐거나 주소가 변경된 초대예요."}</p></div>
      {(invite.copyable || invite.joined) && <a className="dc-invite-copy-button" style={{ minHeight: 44, display: "inline-flex", alignItems: "center" }} href={invite.nativeLink}>{invite.joined ? "내 PC 실행 상태 열기" : "내 PC에서 설정하고 추가"}</a>}
      <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={!invite.copyable || invite.joined} onClick={() => void controls.copy(invite.key)}>참가 안내 복사</button>
    </div>)}
  </section>;
}
