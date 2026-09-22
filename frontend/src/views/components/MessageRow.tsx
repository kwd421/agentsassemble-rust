import type { ReactNode } from "react";
import { Bot, CornerUpLeft, Pin, Zap } from "lucide-react";
import ProviderLogo from "./ProviderLogo";

export function messageTimeLabel(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "--:--";
  }
}

export function MessageAvatar({ avatarImage, providerKind, show = true, system = false }: {
  avatarImage?: string; providerKind?: string; show?: boolean; system?: boolean;
}) {
  return <span className={show ? `dc-message-avatar mt-0.5 ${system ? "system" : "agent"}` : ""}
    data-has-image={Boolean(show && avatarImage && !system)} aria-hidden="true">
    {show && (avatarImage && !system
      ? <img className="dc-message-avatar-image" src={avatarImage} alt="" />
      : system ? <Zap size={16} />
      : <ProviderLogo providerKind={providerKind} size={40} fallback={<Bot size={16} />} />)}
  </span>;
}

export function MessageActions({ onReply, replyDisabled, onTogglePin, pinned, pinDisabled, children }: {
  onReply?: () => void; replyDisabled?: boolean; onTogglePin?: () => void;
  pinned?: boolean; pinDisabled?: boolean; children?: ReactNode;
}) {
  return <div className="dc-message-actions" aria-label="메시지 작업">
    {onReply && <button type="button" className="dc-message-action-button" aria-label="메시지에 답장"
      title="답장" disabled={replyDisabled} onClick={onReply}><CornerUpLeft size={15} /></button>}
    {onTogglePin && <button type="button" className="dc-message-action-button"
      aria-label={pinned ? "메시지 고정 해제" : "메시지 고정"} title={pinned ? "고정 해제" : "메시지 고정"}
      aria-pressed={Boolean(pinned)} disabled={pinDisabled} onClick={onTogglePin}>
      <Pin size={14} fill={pinned ? "currentColor" : "none"} />
    </button>}
    {children}
  </div>;
}

export default function MessageRow({ eventId, author, createdAt, avatarImage, providerKind, role,
  showHeader = true, system = false, selected, actions, reply, children }: {
  eventId: string; author: string; createdAt: string; avatarImage?: string; providerKind?: string;
  role?: string; showHeader?: boolean; system?: boolean; selected?: boolean;
  actions?: ReactNode; reply?: ReactNode; children: ReactNode;
}) {
  return <div className={`dc-message grid grid-cols-[40px_minmax(0,1fr)] gap-3 px-4 ${showHeader ? "py-1.5" : "py-0.5"}`}
    data-room-event-id={eventId} data-role={role || undefined} data-search-target={selected} tabIndex={0}>
    <MessageAvatar avatarImage={avatarImage} providerKind={providerKind} show={showHeader} system={system} />
    {actions}
    <div className="min-w-0">
      {reply}
      {showHeader && <p className="flex items-baseline gap-2">
        <span className="dc-message-author truncate text-[15px] font-semibold text-text-primary preserve-words">{author || "Room"}</span>
        <time dateTime={createdAt} className="shrink-0 text-[11px] text-text-muted">{messageTimeLabel(createdAt)}</time>
      </p>}
      {children}
    </div>
  </div>;
}
