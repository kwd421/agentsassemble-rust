import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AtSign, MessageSquare, Send, Smile } from "lucide-react";
import type { useRoomSideChat } from "../../app/useRoomSideChat";
import type { RoomSocketHandle } from "../../roomSocketTypes";
import type { Mentionable } from "../../lib/mentionComposerModel";
import { MAX_TEXT_CHAT_CHARACTERS } from "../../types/generated/TEXT_CHAT_WIRE";
import DiscordText from "./DiscordText";
import MentionInput from "./MentionInput";

type SideChat = ReturnType<typeof useRoomSideChat>;
const actionStyle = { minWidth: 44, minHeight: 44, display: "inline-flex", alignItems: "center", justifyContent: "center" } as const;

export default function SideChatDock({ chat, socket, canPost, mentionables }: {
  chat: SideChat;
  socket: RoomSocketHandle | null;
  canPost: boolean;
  mentionables: Mentionable[];
}) {
  const [open, setOpen] = useState(false);
  const [sendError, setSendError] = useState({ scope: chat.scope, message: "" });
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const focusAfterSend = useRef(false);
  const selectionAfterInsert = useRef<number | null>(null);
  const currentScope = useRef(chat.scope);
  currentScope.current = chat.scope;
  const value = chat.draft;
  const error = sendError.scope === chat.scope ? sendError.message : "";
  const mentionLabels = useMemo(() => Object.fromEntries(mentionables.map(({ token, label }) => [token, label])), [mentionables]);
  const disabled = !canPost || !chat.snapshot || chat.busy;
  const tooLong = [...value].length > MAX_TEXT_CHAT_CHARACTERS;
  const setValue = chat.updateDraft;
  useEffect(() => {
    if (!chat.busy && focusAfterSend.current) {
      focusAfterSend.current = false;
      inputRef.current?.focus();
    }
  }, [chat.busy, value]);
  useLayoutEffect(() => {
    const cursor = selectionAfterInsert.current;
    if (cursor === null) return;
    selectionAfterInsert.current = null;
    inputRef.current?.focus();
    inputRef.current?.setSelectionRange(cursor, cursor);
  }, [value]);
  function insert(text: string) {
    if (disabled) return;
    const input = inputRef.current;
    const start = input?.selectionStart ?? value.length;
    const end = input?.selectionEnd ?? value.length;
    selectionAfterInsert.current = start + text.length;
    setValue(`${value.slice(0, start)}${text}${value.slice(end)}`);
    input?.focus();
  }
  async function send() {
    if (disabled || !value.trim() || tooLong) return;
    const scope = chat.scope;
    setSendError({ scope, message: "" });
    try {
      await chat.send(value, socket);
      if (currentScope.current !== scope) return;
      chat.updateDraft("");
      focusAfterSend.current = true;
    } catch (cause) {
      if (currentScope.current === scope) setSendError({ scope, message: cause instanceof Error ? cause.message : "메시지를 보내지 못했어요." });
    }
  }
  return <section aria-label="비공식 사이드챗" style={{ flexShrink: 0, borderTop: "1px solid var(--color-panel-soft)", padding: "0 24px", background: "var(--color-panel-bg)" }}>
    <button type="button" aria-expanded={open} aria-label={open ? "사이드챗 접기" : "사이드챗 열기"} onClick={() => setOpen(!open)} style={{ display: "flex", alignItems: "center", gap: 8, minHeight: 44, width: "100%", textAlign: "left" }}>
      <MessageSquare size={17} /><span>사이드챗</span><span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-text-muted)" }}>사람 전용</span>
    </button>
    {open && <div style={{ paddingBottom: 16, display: "flex", flexDirection: "column", gap: 12, maxHeight: "min(46vh, 400px)", overflowY: "auto" }}>
      <p style={{ fontSize: 12 }}>에이전트에게 보이지 않아요. 최근 24시간, 최대 200개를 유지하며 서버를 다시 켜면 사라져요.</p>
      <div className="chat-scroll" aria-label="사이드챗 메시지" style={{ overflowY: "auto", minHeight: 48, maxHeight: 170, flexShrink: 0 }}>
        {chat.snapshot?.messages.map((message) => <article key={message.id} style={{ padding: "8px 0", overflowWrap: "anywhere" }}>
          <p style={{ display: "flex", alignItems: "baseline", gap: 8 }}><strong style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{message.display_name}</strong><time style={{ fontSize: 11, flexShrink: 0 }} dateTime={message.created_at}>{new Date(message.created_at).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit" })}</time></p>
          <DiscordText text={message.content} mentionLabels={mentionLabels} />
        </article>)}
        {chat.snapshot?.messages.length === 0 && <p>아직 메시지가 없어요.</p>}
        {!chat.snapshot && !chat.error && <p role="status">사이드챗 연결을 기다리고 있어요.</p>}
      </div>
      {(chat.error || error) && <p role="alert">{error || chat.error}</p>}
      {chat.error && <button type="button" className="dc-agent-create-secondary" onClick={() => socket?.resync?.()} style={{ minHeight: 44 }}>다시 연결</button>}
      {!canPost && <p style={{ fontSize: 12 }}>이 방에서는 사이드챗을 보기만 할 수 있어요.</p>}
      <MentionInput inputRef={inputRef} value={value} onChange={setValue} disabled={disabled}
        ariaLabel="비공식 사이드챗 입력" maxLength={MAX_TEXT_CHAT_CHARACTERS * 2} placeholder="사이드챗 메시지" mentionables={mentionables}
        onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void send(); } }} />
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexShrink: 0 }}>
        <button type="button" aria-label="사이드챗 멘션 삽입" disabled={disabled} onClick={() => insert("@")} style={actionStyle}><AtSign size={17} /></button>
        <button type="button" aria-label="사이드챗 이모지 삽입" disabled={disabled} onClick={() => insert("🙂")} style={actionStyle}><Smile size={17} /></button>
        <span style={{ marginLeft: "auto", fontSize: 11 }} role={tooLong ? "alert" : undefined}>{[...value].length} / {MAX_TEXT_CHAT_CHARACTERS}</span>
        <button type="button" aria-label="사이드챗 보내기" disabled={disabled || !value.trim() || tooLong} onClick={() => void send()} style={actionStyle}><Send size={17} /></button>
      </div>
    </div>}
  </section>;
}
