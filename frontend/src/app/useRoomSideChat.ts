import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchSideChatSnapshot } from "../api/sideChat";
import type { RoomHttpAuthority } from "../api/roomHttpAuthority";
import { mergeSideChatUpdates } from "../lib/sideChatProjection";
import { parseSideChatUpdate } from "../lib/sideChatContract";
import { SIDE_CHAT_MAX_MESSAGES } from "../types/generated/SIDE_CHAT_WIRE";
import type { SideChatSnapshot } from "../types/generated/SideChatSnapshot";
import type { SideChatUpdate } from "../types/generated/SideChatUpdate";
import type { RoomSocketHandle } from "../roomSocketTypes";

type Connection = {
  scope: object;
  abort: AbortController;
  snapshot: SideChatSnapshot | null;
  buffered: SideChatUpdate[];
  failed: boolean;
  sending: boolean;
};
type View = { scope: object; snapshot: SideChatSnapshot | null; error: string; busy: boolean };

export function useRoomSideChat(roomId: string, authority: RoomHttpAuthority | undefined) {
  const authorityKey = JSON.stringify(authority);
  const scope = useMemo(() => ({ roomId, authority: authorityKey ? JSON.parse(authorityKey) as RoomHttpAuthority : undefined }), [roomId, authorityKey]);
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const connectionRef = useRef<Connection | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [view, setView] = useState<View>({ scope, snapshot: null, error: "", busy: false });
  const current = useCallback((connection: Connection) => currentScope.current === scope && connection.scope === scope && connectionRef.current === connection && !connection.abort.signal.aborted, [scope]);
  const fail = useCallback((connection: Connection, error: unknown) => {
    if (!current(connection)) return;
    connection.failed = true;
    connection.snapshot = null;
    connection.buffered = [];
    setView({ scope, snapshot: null, busy: false, error: error instanceof Error ? error.message : "사이드챗을 불러오지 못했어요." });
  }, [current, scope]);
  const disconnect = useCallback(() => {
    if (currentScope.current !== scope) return;
    connectionRef.current?.abort.abort();
    connectionRef.current = null;
    setView({ scope, snapshot: null, error: "", busy: false });
  }, [scope]);
  useEffect(() => () => {
    connectionRef.current?.abort.abort();
    connectionRef.current = null;
  }, [scope]);

  const connect = useCallback((roomUid: string) => {
    if (currentScope.current !== scope || !scope.roomId || !scope.authority) return;
    connectionRef.current?.abort.abort();
    const connection: Connection = { scope, abort: new AbortController(), snapshot: null, buffered: [], failed: false, sending: false };
    connectionRef.current = connection;
    setView({ scope, snapshot: null, error: "", busy: false });
    void fetchSideChatSnapshot(scope.roomId, roomUid, scope.authority, connection.abort.signal).then((snapshot) => {
      if (!current(connection) || connection.failed) return;
      connection.snapshot = mergeSideChatUpdates(snapshot, connection.buffered);
      connection.buffered = [];
      setView({ scope, snapshot: connection.snapshot, error: "", busy: false });
    }).catch((error: unknown) => fail(connection, error));
  }, [scope, current, fail]);

  const receive = useCallback((update: SideChatUpdate) => {
    const connection = connectionRef.current;
    if (!connection || !current(connection) || connection.failed) return;
    try {
      if (!connection.snapshot) {
        connection.buffered.push(update);
        // Live retention is bounded by its authoritative floor, including HTTP overlap.
        connection.buffered = connection.buffered.filter((item) => item.message.seq > update.retained_after_seq);
        if (connection.buffered.length > SIDE_CHAT_MAX_MESSAGES) throw new Error("사이드챗 초기 기록을 받는 동안 메시지가 밀렸어요. 다시 연결해 주세요.");
      } else {
        connection.snapshot = mergeSideChatUpdates(connection.snapshot, [update]);
        setView((previous) => ({ ...previous, scope, snapshot: connection.snapshot }));
      }
    } catch (error) { fail(connection, error); }
  }, [current, fail, scope]);

  const send = useCallback(async (content: string, socket: RoomSocketHandle | null) => {
    const connection = connectionRef.current;
    if (!connection || !current(connection) || !connection.snapshot || connection.failed || connection.sending || !socket?.ready()) {
      throw new Error("사이드챗 연결이 완료된 뒤 보내 주세요.");
    }
    const snapshot = connection.snapshot;
    connection.sending = true;
    setView((previous) => ({ ...previous, busy: true }));
    try {
      const ack = await socket.command("side_chat.send", { generation: snapshot.generation, after_seq: snapshot.latest_seq, content });
      if (!current(connection) || connection.failed || !connection.snapshot) throw new Error("메시지를 보내는 동안 방 연결이 바뀌었어요.");
      const update = parseSideChatUpdate(ack.result?.update, scope.roomId);
      if (update.generation !== connection.snapshot.generation) throw new Error("사이드챗 기록이 새로 시작됐어요.");
      // ACKs may precede earlier queued live frames. The subscribed stream owns
      // transcript order; the ACK confirms this send without inventing a gap.
      setView((previous) => ({ ...previous, error: "", busy: false }));
    } finally {
      connection.sending = false;
      if (current(connection)) setView((previous) => ({ ...previous, busy: false }));
    }
  }, [current, scope]);
  const visible = view.scope === scope ? view : null;
  const draftKey = visible?.snapshot ? JSON.stringify([roomId, authorityKey, visible.snapshot.room_uid]) : "";
  const updateDraft = (value: string) => {
    if (!draftKey || currentScope.current !== scope) return;
    setDrafts((previous) => {
      const next = { ...previous };
      if (value) next[draftKey] = value; else delete next[draftKey];
      return next;
    });
  };
  return {
    draft: drafts[draftKey] ?? "", updateDraft,
    scope, connect, disconnect, receive, send,
    snapshot: visible?.snapshot ?? null, error: visible?.error ?? "", busy: visible?.busy ?? false,
  };
}
