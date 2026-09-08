import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RoomEvent } from "../api";
import type { RoomSocketHandle } from "../roomSocketTypes";
import type { ChannelHistoryPage } from "../types/generated/ChannelHistoryPage";
import { CHANNEL_HISTORY_PAGE_SIZE, CHANNEL_MESSAGE_EVENT_TYPE } from "../types/generated/TEXT_CHAT_WIRE";
import { ROOM_HISTORY_MAX_EVENTS } from "../types/generated/ROOM_HISTORY_WIRE";

type Window = { events: RoomEvent[]; hasMore: boolean; following: boolean; newMessages: boolean };
type Owner = { scope: object; window: Window | null; buffer: RoomEvent[]; trimmed: boolean; reading: boolean; readVersion: number; sending: boolean; active: boolean };
type View = { scope: object; window: Window | null; loading: boolean; sending: boolean; error: string };
function mergeEvents(older: RoomEvent[], newer: RoomEvent[]) {
  const bySeq = new Map(older.map((event) => [event.seq, event]));
  for (const event of newer) bySeq.set(event.seq, event);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

/** Owns one selected channel's bounded durable window, under the canonical socket's lifetime. */
export function useChannelTranscript({ roomId, roomUid, channelId, socket, connected }: {
  roomId: string; roomUid: string; channelId: string; socket: RoomSocketHandle | null; connected: boolean;
}) {
  const scope = useMemo(() => ({ roomId, roomUid, channelId, socket, connected }), [roomId, roomUid, channelId, socket, connected]);
  const currentScope = useRef(scope); currentScope.current = scope;
  const ownerRef = useRef<Owner | null>(null);
  const [view, setView] = useState<View>({ scope, window: null, loading: false, sending: false, error: "" });
  const current = useCallback((owner: Owner) => currentScope.current === scope && owner.scope === scope && ownerRef.current === owner && owner.active, [scope]);
  const publish = useCallback((owner: Owner, error?: string) => {
    if (current(owner)) setView((previous) => ({ scope, window: owner.window, loading: owner.reading, sending: owner.sending, error: error ?? (previous.scope === scope ? previous.error : "") }));
  }, [current, scope]);
  const read = useCallback(async (owner: Owner, before: number) => {
    if (!current(owner) || !socket?.ready()) return;
    const version = ++owner.readVersion;
    const currentRead = () => current(owner) && owner.readVersion === version;
    owner.reading = true; publish(owner, "");
    try {
      const ack = await socket.command("channel.history", { channel_id: channelId, before_seq: before, limit: CHANNEL_HISTORY_PAGE_SIZE });
      if (!currentRead()) return;
      // The command ACK validator binds the exact room, channel and requested page.
      const page = ack.result as unknown as ChannelHistoryPage;
      const previous = before > 0 ? owner.window?.events ?? [] : [];
      const merged = mergeEvents(page.events, before > 0 ? previous : owner.buffer.filter((event) => event.seq > page.last_seq));
      const events = before > 0 ? merged.slice(0, ROOM_HISTORY_MAX_EVENTS) : merged.slice(-ROOM_HISTORY_MAX_EVENTS);
      owner.window = {
        events, following: before === 0, newMessages: before > 0 && Boolean(owner.window?.newMessages),
        hasMore: page.has_more_before || (before === 0 && (owner.trimmed || merged.length > events.length)),
      };
      owner.buffer = [];
    } catch (error) {
      if (currentRead()) { owner.reading = false; publish(owner, error instanceof Error ? error.message : "채널 기록을 불러오지 못했어요."); }
      return;
    }
    if (currentRead()) { owner.reading = false; publish(owner, ""); }
  }, [channelId, current, publish, socket]);
  const latest = useCallback(() => {
    if (currentScope.current !== scope || !roomId || !roomUid || !channelId || !connected || !socket?.ready()) return;
    if (ownerRef.current?.sending) return;
    if (ownerRef.current) ownerRef.current.active = false;
    const owner: Owner = { scope, window: null, buffer: [], trimmed: false, reading: false, readVersion: 0, sending: false, active: true };
    ownerRef.current = owner;
    void read(owner, 0);
  }, [channelId, connected, read, roomId, roomUid, scope, socket]);
  useEffect(() => {
    latest();
    return () => { if (ownerRef.current) ownerRef.current.active = false; ownerRef.current = null; };
  }, [latest]);

  const receive = useCallback((incoming: RoomEvent[]) => {
    const owner = ownerRef.current;
    if (!owner || !current(owner)) return;
    const events = incoming.filter((event) => event.type === CHANNEL_MESSAGE_EVENT_TYPE && event.message_deleted !== true && event.room_id === roomId && event.channel_id === channelId);
    if (!events.length) return;
    if (!owner.window) {
      const merged = mergeEvents(owner.buffer, events);
      owner.trimmed ||= merged.length > ROOM_HISTORY_MAX_EVENTS;
      owner.buffer = merged.slice(-ROOM_HISTORY_MAX_EVENTS);
      return;
    }
    if (!owner.window.following) owner.window = { ...owner.window, newMessages: true };
    else {
      const merged = mergeEvents(owner.window.events, events);
      owner.window = { ...owner.window, events: merged.slice(-ROOM_HISTORY_MAX_EVENTS), hasMore: owner.window.hasMore || merged.length > ROOM_HISTORY_MAX_EVENTS };
    }
    publish(owner);
  }, [channelId, current, publish, roomId]);
  const earlier = useCallback(async () => {
    const owner = ownerRef.current;
    if (!owner || !current(owner) || owner.reading || !owner.window?.hasMore) return;
    const before = owner.window.events[0]?.seq;
    if (before) {
      owner.window = { ...owner.window, following: false };
      await read(owner, before);
    }
  }, [current, read]);
  const showContext = useCallback((events: RoomEvent[]) => {
    const owner = ownerRef.current;
    if (!owner || !current(owner)) return;
    // Context has its own bounded server projection, without a history paging flag.
    // A pending page must not replace a later explicit context selection.
    owner.readVersion += 1; owner.reading = false; owner.buffer = [];
    owner.window = { events, following: false, hasMore: false, newMessages: false };
    publish(owner, "");
  }, [current, publish]);
  const send = useCallback(async (content: string) => {
    const owner = ownerRef.current;
    if (!owner || !current(owner) || !owner.window || owner.sending || !socket?.ready()) throw new Error("채널 연결이 완료된 뒤 보내 주세요.");
    owner.sending = true; publish(owner);
    try {
      await socket.command("channel.message.send", { channel_id: channelId, content });
      if (!current(owner)) throw new Error("메시지를 보내는 동안 채널 연결이 바뀌었어요.");
      // The canonical stream supplies ordered durable messages, including this ACK's event.
    } finally { owner.sending = false; publish(owner); }
  }, [channelId, current, publish, socket]);
  const visible = view.scope === scope ? view : null;
  return { scope, receive, latest, earlier, showContext, send,
    events: visible?.window?.events ?? [], hasMore: visible?.window?.hasMore ?? false,
    following: visible?.window?.following ?? true, newMessages: visible?.window?.newMessages ?? false,
    ready: Boolean(visible?.window), loading: visible?.loading ?? false,
    sending: visible?.sending ?? false, error: visible?.error ?? "",
  };
}
