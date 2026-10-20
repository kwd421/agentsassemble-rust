import { useEffect, useState } from "react";
import { ShieldQuestion } from "../../ui/icons";

export type DelegatedActionCardProps = {
  /** The AI that is asking. */
  delegateName: string;
  /** What it wants to do, as one sentence ("#기획 채널을 지우려고 해요"). */
  summary: string;
  /** What will happen that the owner should know before allowing it. */
  consequence?: string;
  /** Deleting or removing: the allow button is red. */
  destructive?: boolean;
  /** Starting or creating something with full access needs an explicit tick from the owner. */
  fullAccess?: boolean;
  expiresAt: string;
  busy?: "allow" | "decline";
  error?: string;
  onAllow: (acknowledgedFullAccess: boolean) => void;
  onDecline: () => void;
};

function remaining(expiresAt: string, now: number): string {
  const seconds = Math.max(0, Math.round((Date.parse(expiresAt) - now) / 1000));
  if (seconds >= 60) return `${Math.ceil(seconds / 60)}분 안에 답해 주세요`;
  return seconds > 0 ? `${seconds}초 안에 답해 주세요` : "시간이 지났어요";
}

// A request by the owner's own AI to do something on the owner's behalf. Only the
// owner sees it, and nothing happens until they allow it.
export default function DelegatedActionCard({ delegateName, summary, consequence, destructive, fullAccess, expiresAt, busy, error, onAllow, onDecline }: DelegatedActionCardProps) {
  const [now, setNow] = useState(() => Date.now());
  const [acknowledged, setAcknowledged] = useState(false);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const expired = Date.parse(expiresAt) <= now;
  const locked = Boolean(busy) || expired;
  return (
    <section className="dc-delegated-action" aria-label={`${delegateName}의 확인 요청`}>
      <ShieldQuestion size={24} className="dc-delegated-action-icon" aria-hidden="true" />
      <div className="dc-delegated-action-main">
        <p className="dc-delegated-action-from preserve-words">{delegateName}의 요청</p>
        <p className="dc-delegated-action-summary preserve-words">{summary}</p>
        {consequence && <p className="dc-delegated-action-note preserve-words">{consequence}</p>}
        {fullAccess && (
          <label className="dc-delegated-action-ack">
            <input type="checkbox" checked={acknowledged} disabled={locked} onChange={(event) => setAcknowledged(event.target.checked)} />
            <span>이 컴퓨터 전체에 접근하는 것을 허용해요</span>
          </label>
        )}
        {error
          ? <p className="dc-delegated-action-note" data-tone="error" role="alert">{error}</p>
          : <p className="dc-delegated-action-note">{remaining(expiresAt, now)}</p>}
      </div>
      <div className="dc-delegated-action-buttons">
        <button type="button" className="dc-quiet-button" disabled={locked} onClick={onDecline}>
          {busy === "decline" ? "거절하는 중…" : "거절"}
        </button>
        <button type="button" className="dc-accent-button" data-danger={destructive || undefined}
          disabled={locked || (fullAccess && !acknowledged)} onClick={() => onAllow(acknowledged)}>
          {busy === "allow" ? "허용하는 중…" : "허용"}
        </button>
      </div>
    </section>
  );
}
