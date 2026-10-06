import type { ConnectorInvitePresentation } from "../../app/useManagedAiInvites";

export type ConnectorInviteControls = {
  invites: readonly ConnectorInvitePresentation[];
  creating: boolean;
  create: () => void;
  copy: (key: string) => void;
};

export function ConnectorInviteCard({ controls, disabled, localOnly, mcpOrigin }: { controls: ConnectorInviteControls; disabled: boolean; localOnly: boolean; mcpOrigin?: string }) {
  return <section className="dc-invite-card" aria-labelledby="connector-invite-heading">
    <div className="dc-invite-card-copy">
      <h3 id="connector-invite-heading">사용 중인 AI 앱에서 참가</h3>
      <p>Codex, Claude Code 같은 AI 앱의 대화에 참가 안내를 붙여넣으면 그 AI가 방에 들어와요. 앱에 AgentsAssemble 연결 도구가 있어야 하고, 링크만 열어서는 들어올 수 없어요. 한 번 쓸 수 있고 1시간 뒤 만료돼요.</p>
      {mcpOrigin && <>
        <p>웹 ChatGPT의 설정 → 플러그인 → MCP 앱 만들기에서 서버 URL로 등록해 주세요. 인증은 ‘인증 없음’을 선택해요.</p>
        <code style={{ overflowWrap: "anywhere" }}>{mcpOrigin}/mcp</code>
        <p>이 서버는 앱이 자동으로 준비해요. 등록한 연결 도구를 대화에 추가하고 참가 안내를 전달하면 돼요. 임시 주소가 바뀌면 새 주소로 다시 등록해야 해요.</p>
      </>}
      {localOnly && <p>외부 접속이 꺼져 있어서 이 컴퓨터에서 실행 중인 AI만 쓸 수 있는 초대를 만들어요. 앱을 다시 시작하면 이 초대는 쓸 수 없어요.</p>}
    </div>
    <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }}
      disabled={disabled || controls.creating} onClick={() => controls.create()}>
      {controls.creating ? "초대 만드는 중" : localOnly ? "이 컴퓨터용 초대 만들기" : "초대 만들기"}
    </button>
    {controls.invites.length > 0 && <div role="list">
      {controls.invites.map((invite, index) => <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={invite.key}>
        <div className="dc-invite-card-copy">
          <strong>AI 초대 {controls.invites.length - index}{invite.local ? " · 이 컴퓨터 전용" : ""}</strong>
          <p>{invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "만료되었거나 현재 주소에서 사용할 수 없어요."}</p>
        </div>
        <button type="button" className="dc-invite-row-button" style={{ minWidth: 44, minHeight: 44 }}
          disabled={!invite.copyable} onClick={() => controls.copy(invite.key)}>참가 안내 복사</button>
      </div>)}
    </div>}
  </section>;
}
