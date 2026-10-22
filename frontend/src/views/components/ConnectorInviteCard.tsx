import { useEffect, useRef, useState } from "react";
import type { ConnectorInvitePresentation, ConnectorSeat } from "../../app/useManagedAiInvites";

export type ConnectorInviteControls = {
  invites: readonly ConnectorInvitePresentation[];
  creating: boolean;
  create: () => void;
  /** Makes an invitation that puts an existing external AI back in its own seat. */
  reconnect?: (seat: ConnectorSeat) => void;
  copy: (key: string) => boolean | Promise<boolean> | void;
};

export function ConnectorInviteCard({ controls, disabled, localOnly, mcpOrigin, seats = [] }: { controls: ConnectorInviteControls; disabled: boolean; localOnly: boolean; mcpOrigin?: string; seats?: readonly ConnectorSeat[] }) {
  const [copiedKey, setCopiedKey] = useState("");
  const copiedTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => {
    if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
  }, []);
  return <section className="dc-invite-card" aria-labelledby="connector-invite-heading">
    <div className="dc-invite-card-copy">
      <h3 id="connector-invite-heading">사용 중인 AI 앱에서 참가</h3>
      <p>사용 중인 AI 앱에 참가 안내를 붙여넣어 주세요.<br />초대는 한 번 쓸 수 있고 1시간 뒤 만료돼요.</p>
      <details className="dc-invite-details"><summary>자세히</summary>
      <p>Codex, Claude Code 같은 AI 앱에 AgentsAssemble 연결 도구가 있어야 해요. 링크만 열어서는 참가할 수 없어요.</p>
      {mcpOrigin && <>
        <p>웹 ChatGPT의 설정 → 플러그인 → MCP 앱 만들기에서 서버 URL로 등록해 주세요. 인증은 ‘인증 없음’을 선택해요.</p>
        <code style={{ overflowWrap: "anywhere" }}>{mcpOrigin}/mcp</code>
        <p>이 서버는 앱이 자동으로 준비해요. 등록한 연결 도구를 대화에 추가하고 참가 안내를 전달하면 돼요. 임시 주소가 바뀌면 새 주소로 다시 등록해야 해요.</p>
      </>}
      {localOnly && <p>외부 접속이 꺼져 있어서 이 컴퓨터에서 실행 중인 AI만 쓸 수 있는 초대를 만들어요. 앱을 다시 시작하면 이 초대는 쓸 수 없어요.</p>}
      </details>
    </div>
    <button type="button" className="dc-button dc-button--lg dc-button--primary"
      disabled={disabled || controls.creating} onClick={() => controls.create()}>
      {controls.creating ? "초대 만드는 중" : localOnly ? "이 컴퓨터용 초대 만들기" : "초대 만들기"}
    </button>
    {controls.reconnect && seats.length > 0 && <details className="dc-invite-details">
      <summary>기존 외부 AI 다시 연결</summary>
      <p>새 대화에서 다시 들어오는 외부 AI를 원래 자리로 불러요. 역할, 이름, 프로필 사진이 그대로 남아요.</p>
      <div role="list">
        {seats.map((seat) => <div className="dc-invite-friend-row" role="listitem" key={seat.id}>
          <strong className="preserve-words">{seat.name}</strong>
          <button type="button" className="dc-button dc-button--md dc-button--secondary"
            aria-label={`${seat.name} 다시 연결 초대 만들기`}
            disabled={disabled || controls.creating} onClick={() => controls.reconnect?.(seat)}>다시 연결 초대</button>
        </div>)}
      </div>
    </details>}
    {controls.invites.length > 0 && <div role="list">
      {controls.invites.map((invite, index) => <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={invite.key}>
        <div className="dc-invite-card-copy">
          <strong>{invite.seatName ? `${invite.seatName} 다시 연결` : `AI 초대 ${controls.invites.length - index}`}{invite.local ? " · 이 컴퓨터 전용" : ""}</strong>
          <p>{invite.copyable ? `만료 ${new Date(invite.expiresAt).toLocaleTimeString()}` : "만료되었거나 현재 주소에서 사용할 수 없어요."}</p>
        </div>
        <button type="button" className="dc-button dc-button--lg dc-button--secondary" style={{ minWidth: 44 }}
          disabled={!invite.copyable} onClick={() => {
            void Promise.resolve(controls.copy(invite.key)).then((ok) => {
              if (ok !== true) return;
              setCopiedKey(invite.key);
              if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
              copiedTimer.current = window.setTimeout(() => setCopiedKey(""), 2000);
            });
          }}>{copiedKey === invite.key ? "복사됨" : "참가 안내 복사"}</button>
      </div>)}
    </div>}
  </section>;
}
