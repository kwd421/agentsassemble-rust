import type { RoomEvent } from "../../api";
import type { CanonicalParticipantProfile } from "../../lib/canonicalRoomProjection";
import type { MentionLabels } from "./DiscordText";
import DiscordText from "./DiscordText";
import { buildLobbyRows } from "../lobby/lobbyRows";
import { MessageReply, type ReplySource } from "./MessageReply";
import MessageRow, { MessageActions } from "./MessageRow";

export default function ChannelMessageRows({ events, profiles, mentionLabels, selectedId, pinnedIds,
  canReply, replyDisabled, canPin, pinDisabled, onReply, onTogglePin, replySource, onNavigate }: {
  events: RoomEvent[]; profiles: Record<string, CanonicalParticipantProfile>; mentionLabels: MentionLabels;
  selectedId?: string; pinnedIds: Set<string>; canReply: boolean; replyDisabled: boolean;
  canPin: boolean; pinDisabled: boolean; onReply: (eventId: string) => void;
  onTogglePin: (eventId: string) => void; replySource: (eventId: string) => ReplySource;
  onNavigate: (eventId: string) => void;
}) {
  const byId = new Map(events.map((event) => [event.id, event]));
  const rows = buildLobbyRows(events.map((event) => ({
    id: event.id, created_at: event.created_at, kind: "message", side: "other",
    actor_id: event.actor.participant_id,
    name: profiles[event.actor.participant_id]?.displayName || String(event.display_name || ""),
    message: event.content || "",
  })));
  return <>{rows.map((row) => {
    if (row.type === "divider") return <div className="dc-date-divider px-4" key={row.key} aria-hidden><span>{row.label}</span></div>;
    if (row.type !== "event") return null;
    const event = byId.get(row.event.id)!;
    const profile = profiles[event.actor.participant_id];
    const deleted = event.message_deleted === true;
    const replyId = typeof event.reply_to_event_id === "string" ? event.reply_to_event_id : undefined;
    return <MessageRow key={row.key} eventId={event.id} author={row.event.name} createdAt={event.created_at}
        avatarImage={profile?.avatarImageUrl} providerKind={profile?.providerKind} role={profile?.role}
        showHeader={row.showHeader} selected={selectedId === event.id}
        actions={<MessageActions onReply={canReply && !deleted ? () => onReply(event.id) : undefined}
          replyDisabled={replyDisabled} pinned={pinnedIds.has(event.id)} pinDisabled={pinDisabled}
          onTogglePin={canPin && !deleted ? () => onTogglePin(event.id) : undefined} />}
        reply={!deleted && replyId ? <MessageReply source={replySource(replyId)} onOpen={() => onNavigate(replyId)} /> : undefined}>
        <div className={`text-[14px] leading-relaxed preserve-words ${deleted ? "italic text-text-muted" : "text-text-secondary"}`}>
          {deleted ? "삭제된 메시지입니다" : <DiscordText text={event.content || ""} mentionLabels={mentionLabels} />}
        </div>
      </MessageRow>;
  })}</>;
}
