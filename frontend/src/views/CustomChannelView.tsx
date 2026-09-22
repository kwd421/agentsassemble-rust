import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Hash, Send } from "lucide-react";
import { RoomSocketSayError } from "../roomSocketTypes";
import type { MessagePinsAuthority, RoomChannel, RoomSearchResult } from "../api";
import type { useChannelTranscript } from "../app/useChannelTranscript";
import type { CanonicalParticipantProfile } from "../lib/canonicalRoomProjection";
import type { Mentionable } from "../lib/mentionComposerModel";
import { MAX_TEXT_CHAT_CHARACTERS } from "../types/generated/TEXT_CHAT_WIRE";
import ChannelHeader, { type ChannelHeaderActions, type ChannelSearchScope } from "./components/ChannelHeader";
import ChannelMessageRows from "./components/ChannelMessageRows";
import MentionInput from "./components/MentionInput";
import "./CustomChannelView.css";
import { ReplyDraft } from "./components/MessageReply";
import { useMessagePins } from "./useMessagePins";
import type { RoomMessageSearchController } from "./useRoomMessageSearch";

type Transcript = ReturnType<typeof useChannelTranscript>;

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
  const selection = useMemo(() => ({ identity }), [identity]);
  const selectionRef = useRef(selection); selectionRef.current = selection;
  const scopeRef = useRef(transcript.scope); scopeRef.current = transcript.scope;
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const [draft, setDraft] = useState<{ identity: string; value: string; replyTo?: string; retry?: RoomSocketSayError["retry"] }>({ identity, value: "" });
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
  const replyTo = draft.identity === identity ? draft.replyTo : undefined;
  function replySource(eventId: string) {
    const source = transcript.events.find((event) => event.id === eventId);
    return { eventId, author: source ? participantProfiles[source.actor.participant_id]?.displayName || source.display_name || undefined : undefined,
      text: source?.content || undefined, deleted: source?.message_deleted === true };
  }
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
    const element = Array.from(node.querySelectorAll<HTMLElement>("[data-room-event-id]")).find((item) => item.dataset.roomEventId === target.id);
    if (element) {
      pendingFocus.current = null;
      element.scrollIntoView({ block: "center" }); element.focus({ preventScroll: true });
      setAtBottom(node.scrollHeight - node.scrollTop - node.clientHeight < 24);
    }
  }, [transcript.events, transcript.loading, transcript.following, transcript.scope, atBottom, selected]);
  useEffect(() => {
    if (!transcript.sending && focusAfterSend.current) { focusAfterSend.current = false; inputRef.current?.focus(); }
  }, [transcript.sending, value]);

  async function navigate(eventId: string, requireContext = false) {
    if (!channel || !transcript.scope.connected) { setSendError({ identity, message: "채널 연결이 완료된 뒤 메시지를 열어 주세요." }); return; }
    const scope = transcript.scope;
    if (!requireContext && transcript.events.some((event) => event.id === eventId)) {
      const target = { scope, id: eventId };
      pendingFocus.current = target; setSelected(target);
      return;
    }
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
    void navigate(pendingSearchTargetEventId, true).finally(() => { if (active) onSearchTargetHandled?.(); });
    return () => { active = false; };
  }, [pendingSearchTargetEventId, transcript.scope, channel?.id]);

  async function send() {
    if (disabled || !value.trim() || tooLong) return;
    setSendError({ identity, message: "" });
    try {
      if (replyTo) await transcript.send(value, draft.retry, replyTo);
      else if (draft.retry) await transcript.send(value, draft.retry);
      else await transcript.send(value);
      if (!mounted.current || selectionRef.current !== selection) return;
      setDraft({ identity, value: "" }); focusAfterSend.current = true;
    } catch (cause) {
      if (mounted.current && selectionRef.current === selection) {
        setSendError({ identity, message: cause instanceof Error ? cause.message : "메시지를 보내지 못했어요." });
        if (cause instanceof RoomSocketSayError && cause.retry) {
          setDraft((current) => current.identity === identity && current.value === value
            ? { ...current, retry: cause.retry } : current);
        }
      }
    }
  }
  const searchItems = messageSearch.results.map((result) => ({
    id: result.event_id, author: participantProfiles[result.participant_id]?.displayName || result.author,
    avatarImage: participantProfiles[result.participant_id]?.avatarImageUrl,
    body: result.content || result.attachment_filenames.join(", "),
    meta: `${messageSearchScope === "all" ? `#${messageSearchChannelLabels[result.channel_id] || result.channel_id} · ` : ""}${new Date(result.created_at).toLocaleString("ko-KR")}`,
    onSelect: () => result.channel_id === channelId ? void navigate(result.event_id, true) : onOpenCrossChannelSearchResult(result),
  }));
  const latestSeq = transcript.events.at(-1)?.seq;
  return <div className="dc-custom-channel flex min-h-0 min-w-0 flex-1 flex-col">
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
    <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-y-auto py-4 chat-scroll" style={{ overflowAnchor: "none" }} aria-label="채널 메시지"
      onScroll={(event) => { const node = event.currentTarget; setAtBottom(node.scrollHeight - node.scrollTop - node.clientHeight < 24); }}>
      {transcript.hasMore && <button type="button" className="dc-channel-history-button" disabled={transcript.loading} onClick={() => {
        const node = scrollRef.current; if (node) restoreScroll.current = { height: node.scrollHeight, top: node.scrollTop };
        setAtBottom(false); void transcript.earlier();
      }}>이전 메시지</button>}
      {!transcript.ready && !transcript.error && <p className="px-4 text-[13px] text-text-muted" role="status">채널 연결과 기록을 기다리고 있어요.</p>}
      {transcript.ready && transcript.following && !transcript.hasMore && <section className="dc-channel-intro px-4 pb-5 pt-2">
        <span className="dc-channel-intro-icon"><Hash size={26} /></span>
        <h2 className="mt-3 text-[28px] font-black leading-tight text-text-primary preserve-words">{channel?.name}</h2>
        <p className="mt-1 text-[14px] leading-relaxed text-text-muted">이 채널의 대화가 시작되는 곳이에요.</p>
      </section>}
      {transcript.ready && !transcript.following && <p className="px-4 pb-3 text-center text-[12px] text-text-muted">검색한 메시지 주변 기록</p>}
      <ChannelMessageRows events={transcript.events} profiles={participantProfiles} mentionLabels={mentionLabels}
        selectedId={selected?.scope === transcript.scope ? selected.id : undefined} pinnedIds={pinnedIds}
        canReply={canPost} replyDisabled={disabled} canPin={Boolean(canPin && authority)}
        pinDisabled={pins.pinsLoading || pins.pinBusyIds.size > 0}
        onReply={(id) => { setDraft({ identity, value, replyTo: id }); inputRef.current?.focus(); }}
        onTogglePin={(id) => void pins.setPinned(id, !pinnedIds.has(id))}
        replySource={replySource} onNavigate={(id) => void navigate(id)} />
    </div>
    <div className="relative shrink-0 px-4 pb-5">
      {(!transcript.following || !atBottom) && <div className="dc-old-history-notice" role="status">
        <span>{transcript.newMessages ? "새 메시지가 있어요" : "오래된 메시지를 보고 있어요"}</span>
        <button type="button" disabled={transcript.sending} onClick={() => {
          pendingFocus.current = null; setSelected(null); setAtBottom(true);
          if (!transcript.following) transcript.latest();
          else if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }} aria-label={transcript.newMessages ? "새 메시지 · 최신으로" : "최신 메시지"}>최근으로 이동하기</button>
      </div>}
      {transcript.error && <div role="alert" className="dc-channel-notice">{transcript.error} <button type="button" disabled={transcript.sending} onClick={transcript.latest}>다시 불러오기</button></div>}
      <section className="dc-composer-shell">
        {replyTo && <ReplyDraft source={replySource(replyTo)} onCancel={() => setDraft({ identity, value })} />}
        {(error || pins.pinsError || tooLong) && <p role="alert" className="dc-channel-notice">{error || pins.pinsError || `메시지는 ${MAX_TEXT_CHAT_CHARACTERS}자까지 보낼 수 있어요.`}</p>}
        {channel && transcript.ready && !canPost && <p className="dc-composer-readonly">이 채널에서는 보기만 할 수 있어요.</p>}
        <div className="dc-composer-bar">
          <MentionInput inputRef={inputRef} className="dc-composer-input" ariaLabel="채널 메시지 입력"
            placeholder={!channel || !transcript.ready ? "채널 연결을 기다리고 있어요" : canPost ? `#${channel.name}에 메시지 보내기` : "이 채널에서는 보기만 할 수 있어요"}
            value={value} disabled={disabled} maxLength={MAX_TEXT_CHAT_CHARACTERS * 2} mentionables={mentionables}
            onChange={(text) => setDraft({ identity, value: text, replyTo })}
            onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(); } }} />
          <button type="button" className="dc-composer-button send" data-role="send" aria-label="채널 메시지 보내기" title="메시지 보내기"
            disabled={disabled || !value.trim() || tooLong} onClick={() => void send()}><Send size={17} /></button>
        </div>
      </section>
    </div>
  </div>;
}
