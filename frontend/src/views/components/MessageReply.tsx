import { CornerUpLeft, X } from "lucide-react";

export type ReplySource = { eventId: string; author?: string; text?: string; deleted?: boolean };

export function MessageReply({ source, onOpen }: { source: ReplySource; onOpen?: () => void }) {
  return <button type="button" onClick={onOpen} disabled={!onOpen || source.deleted}
    className="dc-reply-preview"
    aria-label="답장 원문 보기" title={source.deleted ? "삭제된 메시지" : source.text || "원문 보기"}>
    {source.author && <span className="dc-reply-preview-author">@{source.author}</span>}
    <span className={`dc-reply-preview-text truncate ${source.deleted ? "italic" : ""}`}>{source.deleted ? "삭제된 메시지예요" : source.text || "원문 보기"}</span>
  </button>;
}

export function ReplyDraft({ source, onCancel }: { source: ReplySource; onCancel: () => void }) {
  return <div className="dc-reply-draft" role="status">
    <CornerUpLeft size={16} className="shrink-0" />
    <span className="min-w-0 flex-1 truncate">{source.author ? `${source.author}에게 답장` : "메시지에 답장"}
      {source.deleted ? " · 삭제된 메시지예요" : source.text ? ` · ${source.text}` : ""}</span>
    <button type="button" aria-label="답장 취소" onClick={onCancel} className="dc-message-action-button"><X size={16} /></button>
  </div>;
}
