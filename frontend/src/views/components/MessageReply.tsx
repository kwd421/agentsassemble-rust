import { Bot, X } from "../../ui/icons";
import ParticipantAvatarContent from "./ParticipantAvatarContent";
import ResourceImage from "./ResourceImage";

export type ReplyAvatar = { image?: string; label?: string; participantType?: string; providerKind?: string };
/** `mentioned`: the reply calls the original author, which Discord marks with `@`. */
export type ReplySource = { eventId: string; author?: string; text?: string; deleted?: boolean; avatar?: ReplyAvatar; mentioned?: boolean };

/** Whether a reply's own text mentions the author of the message it answers. */
export function replyMentions(replyText: string | undefined, authorId: string | undefined): boolean {
  return Boolean(replyText && authorId && replyText.includes(`<@${authorId}>`));
}

export function MessageReply({ source, onOpen }: { source: ReplySource; onOpen?: () => void }) {
  return <button type="button" onClick={onOpen} disabled={!onOpen || source.deleted}
    className="dc-reply-preview"
    aria-label="답장 원문 보기" title={source.deleted ? "삭제된 메시지" : source.text || "원문 보기"}>
    {source.author && source.avatar && <span className="dc-reply-preview-avatar" aria-hidden="true">
      {source.avatar.image
        ? <ResourceImage src={source.avatar.image} alt="" />
        : <ParticipantAvatarContent participantType={source.avatar.participantType} displayName={source.author}
            avatarLabel={source.avatar.label} providerKind={source.avatar.providerKind} size={12} fallback={<Bot size={10} />} />}
    </span>}
    {source.author && <span className="dc-reply-preview-author">{source.mentioned ? "@" : ""}{source.author}</span>}
    <span className={`dc-reply-preview-text truncate ${source.deleted ? "italic" : ""}`}>{source.deleted ? "삭제된 메시지예요" : source.text || "원문 보기"}</span>
  </button>;
}

export function ReplyDraft({ source, onCancel, mention, onToggleMention }: {
  source: ReplySource; onCancel: () => void;
  /** Discord's "@ on/off": shown only where the host can carry it. */
  mention?: boolean; onToggleMention?: () => void;
}) {
  return <div className="dc-reply-draft" role="status">
    <span className="min-w-0 flex-1 truncate">{source.author
      ? <><strong className="font-semibold text-text-normal">{source.author}</strong>님에게 답장하는 중</>
      : "메시지에 답장하는 중"}</span>
    {onToggleMention && <>
      <button type="button" className="dc-reply-draft-mention" aria-pressed={Boolean(mention)} onClick={onToggleMention}
        title={mention ? "답장할 때 상대를 불러요" : "답장만 하고 부르지 않아요"}>@ {mention ? "켜짐" : "꺼짐"}</button>
      <span className="dc-reply-draft-divider" aria-hidden="true" />
    </>}
    <button type="button" aria-label="답장 취소" onClick={onCancel} className="dc-reply-draft-close"><X size={11} strokeWidth={3.5} /></button>
  </div>;
}
