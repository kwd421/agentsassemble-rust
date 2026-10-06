import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";

export default function CompanionInviteCard({ controls, providers }: { controls: CompanionInviteControls; providers: Pick<NativeCliProviderAvailability, "id" | "display_name">[] }) {
  return <section className="dc-invite-card" style={{ margin: "20px 24px", minWidth: 0 }} aria-label="AI 추가">
    <div className="dc-invite-card-copy"><h3>AI 추가</h3><p>이 컴퓨터에서 실행한 AI가 함께 참가해요. 내가 방을 나가면 AI도 더 이상 대화에 참여할 수 없어요.</p></div>
    <label style={{ display: "grid", gap: 8 }}>AI 이름
      <input className="ops-input" style={{ minHeight: 44, minWidth: 0 }} maxLength={80} value={controls.displayName} disabled={controls.creating} onChange={(event) => controls.setDisplayName(event.target.value)} />
    </label>
    <label style={{ display: "grid", gap: 8 }}>AI 선택
      <select aria-label="AI 종류" className="ops-input" style={{ minHeight: 44 }} value={controls.provider} disabled={controls.creating}
        onChange={(event) => controls.setProvider(event.currentTarget.value)}><option value="">AI 선택</option>
        {providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.display_name}</option>)}
      </select>
    </label>
    <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={controls.creating || !controls.provider.trim() || !controls.displayName.trim()} onClick={() => void controls.create((link) => window.location.assign(link))}>{controls.creating ? "초대 만드는 중" : "설정 계속하기"}</button>
    {controls.status && <p role="status">{controls.status}</p>}
    {controls.invites.map((invite) => <div key={invite.key} className="dc-invite-friend-row" style={{ flexWrap: "wrap" }}>
      <div className="dc-invite-card-copy"><strong>{invite.displayName} · {invite.provider}</strong><p>{invite.joined ? "방 참가가 확인됐어요. 실행 상태는 AI 목록에서 확인해 주세요." : invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "만료됐거나 주소가 변경된 초대예요."}</p></div>
      {(invite.copyable || invite.joined) && <a className="dc-invite-copy-button" style={{ minHeight: 44, display: "inline-flex", alignItems: "center" }} href={invite.nativeLink}>{invite.joined ? "실행 상태 열기" : "이 컴퓨터에서 설정하기"}</a>}
      <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={!invite.copyable || invite.joined} onClick={() => void controls.copy(invite.key)}>참가 안내 복사</button>
    </div>)}
  </section>;
}
