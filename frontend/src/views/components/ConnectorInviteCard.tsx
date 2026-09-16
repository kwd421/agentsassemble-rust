import type { ConnectorInvitePresentation } from "../../app/useManagedAiInvites";

export type ConnectorInviteControls = {
  invites: readonly ConnectorInvitePresentation[];
  creating: boolean;
  create: () => void;
  copy: (key: string) => void;
};

export function ConnectorInviteCard({ controls, disabled, localOnly }: { controls: ConnectorInviteControls; disabled: boolean; localOnly: boolean }) {
  return <section className="dc-invite-card" aria-labelledby="connector-invite-heading">
    <div className="dc-invite-card-copy">
      <h3 id="connector-invite-heading">현재 AI 대화 초대</h3>
      <p>Room Connector를 연결한 AI 대화에 링크를 전달해 주세요. 한 번 사용할 수 있고 1시간 뒤 만료돼요.</p>
      {localOnly && <p>외부 접속이 꺼져 있어 이 PC에서 실행 중인 AI만 열 수 있는 링크를 만들어요. 앱을 다시 시작하면 이 링크는 더 이상 열리지 않아요.</p>}
    </div>
    <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }}
      disabled={disabled || controls.creating} onClick={() => controls.create()}>
      {controls.creating ? "초대 만드는 중" : localOnly ? "이 PC의 AI 초대 만들기" : "외부 AI 초대 만들기"}
    </button>
    {controls.invites.length > 0 && <div role="list">
      {controls.invites.map((invite, index) => <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={invite.key}>
        <div className="dc-invite-card-copy">
          <strong>외부 AI 초대 {controls.invites.length - index}{invite.local ? " · 이 PC 전용" : ""}</strong>
          <p>{invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "만료되었거나 현재 주소에서 사용할 수 없어요."}</p>
        </div>
        <button type="button" className="dc-invite-copy-button" style={{ minWidth: 44, minHeight: 44 }}
          disabled={!invite.copyable} onClick={() => controls.copy(invite.key)}>초대 복사</button>
      </div>)}
    </div>}
  </section>;
}
