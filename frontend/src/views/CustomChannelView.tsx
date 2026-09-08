import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Hash, Pin, Send } from "lucide-react";
import type { MessagePinsAuthority, RoomChannel, RoomSearchResult } from "../api";
import type { useChannelTranscript } from "../app/useChannelTranscript";
import type { CanonicalParticipantProfile } from "../lib/canonicalRoomProjection";
import type { Mentionable } from "../lib/mentionComposerModel";
import { MAX_TEXT_CHAT_CHARACTERS } from "../types/generated/TEXT_CHAT_WIRE";
import ChannelHeader, { type ChannelHeaderActions, type ChannelSearchScope } from "./components/ChannelHeader";
import DiscordText from "./components/DiscordText";
import { useMessagePins } from "./useMessagePins";
import type { RoomMessageSearchController } from "./useRoomMessageSearch";
import "../styles/custom-channel.css";

type Transcript = ReturnType<typeof useChannelTranscript>;
const buttonStyle = { minWidth: 44, minHeight: 44 };

export default function CustomChannelView({
  channel, channelId, roomId, roomUid, transcript, authority, messageSearch,
  canPost, canPin, participantProfiles = {}, mentionables = [], searchLabel,
  membersOpen, onToggleMembers, onOpenMobileSidebar, onOpenMobileInfo, headerActions,
  messageSearchScope = "channel", onMessageSearchScopeChange, messageSearchChannelLabels = {},
  pendingSearchTargetEventId = "", onSearchTargetHandled, onOpenCrossChannelSearchResult,
}: {
  channel: RoomChannel | null; channelId: string; roomId: string; roomUid: string;
  transcript: Transcript; authority?: MessagePinsAuthority; messageSearch: RoomMessageSearchController;
  canPost: boolean; canPin: boolean; participantProfiles?: Record<string, CanonicalParticipantProfile>;
  mentionables?: Mentionable[]; searchLabel?: string; membersOpen?: boolean;
  onToggleMembers?: () => void; onOpenMobileSidebar?: () => void; onOpenMobileInfo?: () => void;
  headerActions?: ChannelHeaderActions; messageSearchScope?: ChannelSearchScope;
  onMessageSearchScopeChange?: (scope: ChannelSearchScope) => void;
  messageSearchChannelLabels?: Record<string, string>; pendingSearchTargetEventId?: string;
  onSearchTargetHandled?: () => void; onOpenCrossChannelSearchResult: (result: RoomSearchResult) => void;
}) {
  const identity = JSON.stringify([roomId, roomUid, channelId, authority]);
  const identityRef = useRef(identity); identityRef.current = identity;
  const scopeRef = useRef(transcript.scope); scopeRef.current = transcript.scope;
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const [draft, setDraft] = useState({ identity, value: "" });
  const [sendError, setSendError] = useState({ identity, message: "" });
  const [selected, setSelected] = useState<{ scope: object; id: string } | null>(null);
  const pendingFocus = useRef<typeof selected>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const restoreScroll = useRef<{ height: number; top: number } | null>(null);
  const [atBottom, setAtBottom] = useState(true);
  const focusAfterSend = useRef(false);
  const pins = useMessagePins({ roomId, roomUid, channelId, authority });
  const pinnedIds = useMemo(() => new Set(pins.pinnedItems.map((pin) => pin.event_id)), [pins.pinnedItems]);
  const mentionLabels = useMemo(() => Object.fromEntries(mentionables.map(({ token, label }) => [token, label])), [mentionables]);
  const value = draft.identity === identity ? draft.value : "";
  const error = sendError.identity === identity ? sendError.message : "";
  const tooLong = [...value].length > MAX_TEXT_CHAT_CHARACTERS;
  const disabled = !channel || !canPost || !transcript.ready || transcript.sending;

  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!node) return;
    if (restoreScroll.current && !transcript.loading) {
      const previous = restoreScroll.current; restoreScroll.current = null;
      node.scrollTop = previous.top + node.scrollHeight - previous.height;
    } else if (transcript.following && atBottom) node.scrollTop = node.scrollHeight;
    const target = pendingFocus.current;
    if (target?.scope !== transcript.scope) return;
    const element = Array.from(node.querySelectorAll<HTMLElement>("[data-channel-event-id]")).find((item) => item.dataset.channelEventId === target.id);
    if (element) { pendingFocus.current = null; element.scrollIntoView({ block: "center" }); element.focus({ preventScroll: true }); }
  }, [transcript.events, transcript.loading, transcript.following, transcript.scope, atBottom, selected]);
  useEffect(() => {
    if (!transcript.sending && focusAfterSend.current) { focusAfterSend.current = false; inputRef.current?.focus(); }
  }, [transcript.sending, value]);

  async function navigate(eventId: string) {
    if (!channel || !transcript.scope.connected) { setSendError({ identity, message: "채널 연결이 완료된 뒤 메시지를 열어 주세요." }); return; }
    const scope = transcript.scope;
    try {
      const context = await messageSearch.readContext(eventId, channelId);
      if (!context || !mounted.current || scopeRef.current !== scope) return;
      const target = { scope, id: eventId }; pendingFocus.current = target; setSelected(target);
      setAtBottom(false); restoreScroll.current = null;
      transcript.showContext(context.events);
    } catch (cause) {
      if (mounted.current && scopeRef.current === scope) setSendError({ identity, message: cause instanceof Error ? cause.message : "메시지를 열지 못했어요." });
    }
  }
  useEffect(() => {
    if (!pendingSearchTargetEventId || !channel || !transcript.scope.connected) return;
    let active = true;
    void navigate(pendingSearchTargetEventId).finally(() => { if (active) onSearchTargetHandled?.(); });
    return () => { active = false; };
  }, [pendingSearchTargetEventId, transcript.scope, channel?.id]);

  async function send() {
    if (disabled || !value.trim() || tooLong) return;
    setSendError({ identity, message: "" });
    try {
      await transcript.send(value);
      if (!mounted.current || identityRef.current !== identity) return;
      setDraft({ identity, value: "" }); focusAfterSend.current = true;
    } catch (cause) {
      if (mounted.current && identityRef.current === identity) setSendError({ identity, message: cause instanceof Error ? cause.message : "메시지를 보내지 못했어요." });
    }
  }
  const searchItems = messageSearch.results.map((result) => ({
    id: result.event_id, author: participantProfiles[result.participant_id]?.displayName || result.author,
    avatarImage: participantProfiles[result.participant_id]?.avatarImageUrl,
    body: result.content || result.attachment_filenames.join(", "),
    meta: `${messageSearchScope === "all" ? `#${messageSearchChannelLabels[result.channel_id] || result.channel_id} · ` : ""}${new Date(result.created_at).toLocaleString("ko-KR")}`,
    onSelect: () => result.channel_id === channelId ? void navigate(result.event_id) : onOpenCrossChannelSearchResult(result),
  }));
  const latestSeq = transcript.events.at(-1)?.seq;
  return <div className="flex min-h-0 min-w-0 flex-1 flex-col">
    <ChannelHeader icon={<Hash size={20} />} title={channel?.name ?? "채널 연결 중"} searchLabel={searchLabel}
      membersOpen={membersOpen} onToggleMembers={onToggleMembers} onOpenMobileSidebar={onOpenMobileSidebar} onOpenMobileInfo={onOpenMobileInfo}
      headerActions={{ ...headerActions, pinnedItems: pins.pinnedItems, pinsLoading: pins.pinsLoading, pinsError: pins.pinsError, latestReadCursor: latestSeq ? `seq:${latestSeq}` : "",
        onOpenPins: authority ? () => void pins.reloadPins() : undefined,
        onSelectPin: (pin) => void navigate(pin.event_id),
        onUnpin: canPin && authority && !pins.pinBusyIds.size ? (pin) => void pins.setPinned(pin.event_id, false) : undefined }}
      searchItems={searchItems} externalSearch searchQuery={messageSearch.query} searchScope={messageSearchScope}
      onSearchScopeChange={onMessageSearchScopeChange} searchLoading={messageSearch.loading} searchError={messageSearch.error}
      onSearchQueryChange={messageSearch.updateQuery} searchHasMore={messageSearch.hasMore} searchLoadingMore={messageSearch.loadingMore}
      onLoadMoreSearch={() => void messageSearch.loadMore()} />
    <div ref={scrollRef} className="chat-scroll" style={{ minHeight: 0, flex: 1, overflowY: "auto", padding: "16px 24px" }} aria-label="채널 메시지"
      onScroll={(event) => { const node = event.currentTarget; setAtBottom(node.scrollHeight - node.scrollTop - node.clientHeight < 24); }}>
      {transcript.hasMore && <button type="button" className="ops-button" style={buttonStyle} disabled={transcript.loading} onClick={() => {
        const node = scrollRef.current; if (node) restoreScroll.current = { height: node.scrollHeight, top: node.scrollTop };
        setAtBottom(false); void transcript.earlier();
      }}>이전 메시지</button>}
      {!transcript.ready && !transcript.error && <p role="status">채널 연결과 기록을 기다리고 있어요.</p>}
      {transcript.ready && transcript.events.length === 0 && <p className="text-text-muted">첫 메시지를 남겨보세요.</p>}
      {transcript.events.map((event) => <article key={event.id} data-channel-event-id={event.id} data-search-target={selected?.scope === transcript.scope && selected.id === event.id} tabIndex={-1}
        className="dc-channel-message" style={{ padding: "8px 0", overflowWrap: "anywhere" }}>
        <div className="dc-channel-message-author-line">
          <strong className="min-w-0 flex-1 preserve-words">{participantProfiles[event.actor.participant_id]?.displayName || event.display_name}</strong>
          <time dateTime={event.created_at} style={{ fontSize: 11, color: "var(--color-text-muted)" }}>{new Date(event.created_at).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" })}</time>
          {canPin && authority && <button type="button" className="dc-channel-message-pin" style={{ ...buttonStyle, opacity: 1 }} aria-label={pinnedIds.has(event.id) ? "고정 해제" : "메시지 고정"}
            data-pinned={pinnedIds.has(event.id)} disabled={pins.pinsLoading || pins.pinBusyIds.size > 0} onClick={() => void pins.setPinned(event.id, !pinnedIds.has(event.id))}><Pin size={16} /></button>}
        </div>
        <DiscordText text={event.content || ""} mentionLabels={mentionLabels} />
      </article>)}
    </div>
    {(!transcript.following || !atBottom) && <button type="button" className="ops-button" style={{ ...buttonStyle, margin: "0 24px 12px" }} disabled={transcript.sending} onClick={() => {
      pendingFocus.current = null; setSelected(null); setAtBottom(true);
      if (!transcript.following) transcript.latest();
      else if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }}>{transcript.newMessages ? "새 메시지 · 최신으로" : "최신 메시지"}</button>}
    {transcript.error && <div role="alert" style={{ padding: "0 24px 12px" }}>{transcript.error} <button type="button" className="ops-button" style={buttonStyle} disabled={transcript.sending} onClick={transcript.latest}>다시 불러오기</button></div>}
    <div style={{ padding: "12px 24px 16px", display: "flex", gap: 12, alignItems: "flex-end" }}>
      <textarea ref={inputRef} className="ops-input" style={{ minHeight: 44, maxHeight: 120, minWidth: 0, flex: 1, resize: "vertical" }} rows={2}
        aria-label="채널 메시지 입력" placeholder={!channel || !transcript.ready ? "채널 연결을 기다리고 있어요" : canPost ? "메시지 보내기" : "이 채널에서는 보기만 할 수 있어요"}
        value={value} disabled={disabled} maxLength={MAX_TEXT_CHAT_CHARACTERS * 2} onChange={(event) => setDraft({ identity, value: event.target.value })}
        onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(); } }} />
      <button type="button" className="ops-cta" style={buttonStyle} aria-label="채널 메시지 보내기" disabled={disabled || !value.trim() || tooLong} onClick={() => void send()}><Send size={18} /></button>
    </div>
    {(error || pins.pinsError || tooLong) && <p role="alert" style={{ padding: "0 24px 16px" }}>{error || pins.pinsError || `메시지는 ${MAX_TEXT_CHAT_CHARACTERS}자까지 보낼 수 있어요.`}</p>}
  </div>;
}
