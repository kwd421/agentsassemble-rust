import { useState } from "react";
import { Copy, Link2 } from "lucide-react";
import type { DelegatedInviteAck } from "../../types/generated/DelegatedInviteAck";
import { humanInviteSummary } from "../../lib/humanInviteSummary";

// The link an approved invite request produced. It is shown once, to the owner who
// approved it, and is gone when this card closes.
export default function DelegatedInviteLink({ invite, onClose }: { invite: DelegatedInviteAck; onClose: () => void }) {
  const [copied, setCopied] = useState<"" | "done" | "failed">("");
  const ttlSeconds = Math.max(0, Math.round((Date.parse(invite.expires_at) - Date.now()) / 1000));
  async function copy() {
    try { await navigator.clipboard.writeText(invite.join_url); setCopied("done"); }
    catch { setCopied("failed"); }
  }
  return (
    <section className="dc-delegated-action" aria-label="만든 초대 링크">
      <Link2 size={24} className="dc-delegated-action-icon" aria-hidden="true" />
      <div className="dc-delegated-action-main">
        <p className="dc-delegated-action-summary preserve-words">초대 링크를 만들었어요</p>
        <p className="dc-delegated-action-note preserve-words">
          {humanInviteSummary(invite.max_uses, Math.round(ttlSeconds / 60) * 60)} 닫으면 다시 볼 수 없어요.
        </p>
        <div className="dc-invite-link-field">
          <input className="dc-invite-link-input" value={invite.join_url} readOnly aria-label="만든 초대 링크" onFocus={(event) => event.currentTarget.select()} />
          <button type="button" className="dc-invite-copy-button" onClick={() => void copy()}>
            <Copy size={15} />
            {copied === "done" ? "복사했어요" : "복사"}
          </button>
        </div>
        {copied === "failed" && <p className="dc-delegated-action-note" data-tone="error" role="alert">복사하지 못했어요. 링크를 직접 선택해 주세요.</p>}
      </div>
      <div className="dc-delegated-action-buttons">
        <button type="button" className="dc-quiet-button" onClick={onClose}>닫기</button>
      </div>
    </section>
  );
}
