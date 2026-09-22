import { CornerUpLeft, X } from "lucide-react";

export type ReplySource = { eventId: string; author?: string; text?: string; deleted?: boolean };

export function MessageReply({ source, onOpen }: { source: ReplySource; onOpen?: () => void }) {
  return <button type="button" onClick={onOpen} disabled={!onOpen || source.deleted}
    className="mb-1 flex max-w-full items-center gap-2 text-left text-[12px] text-text-muted"
    aria-label="답장 원문 보기" title={source.deleted ? "삭제된 메시지" : source.text || "원문 보기"}>
    <CornerUpLeft size={14} className="shrink-0" />
    {source.author && <strong className="shrink-0">{source.author}</strong>}
    <span className="truncate">{source.deleted ? "삭제된 메시지입니다" : source.text || "원문 보기"}</span>
  </button>;
}

export function ReplyDraft({ source, onCancel }: { source: ReplySource; onCancel: () => void }) {
  return <div className="mb-2 flex items-center gap-2 rounded bg-white/5 px-3 py-2 text-[12px]" role="status">
    <CornerUpLeft size={14} className="shrink-0" />
    <span className="min-w-0 flex-1 truncate">{source.author ? `${source.author}에게 답장` : "메시지에 답장"}
      {source.deleted ? " · 삭제된 메시지입니다" : source.text ? ` · ${source.text}` : ""}</span>
    <button type="button" aria-label="답장 취소" onClick={onCancel} className="dc-message-action-button"><X size={16} /></button>
  </div>;
}
