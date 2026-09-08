import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { fetchMessagePins, setMessagePinned, type MessagePin, type MessagePinsAuthority } from "../api";

type Mutation = { eventId: string; pinned: boolean };
type PinView = { scope: object; items: MessagePin[]; loading: boolean; busyId: string; error: string };

/** One explicit channel's on-demand pin operations; no background refresh or cache. */
export function useMessagePins({ roomId, roomUid = "", channelId, authority }: {
  roomId: string; roomUid?: string; channelId: string; authority?: MessagePinsAuthority;
}) {
  const authorityKey = JSON.stringify(authority ?? null);
  const scope = useMemo(() => ({ roomId, roomUid, channelId, authority, active: true }), [roomId, roomUid, channelId, authorityKey]);
  const currentScope = useRef(scope); currentScope.current = scope;
  const operationRef = useRef<object | null>(null);
  const [view, setView] = useState<PinView>({ scope, items: [], loading: false, busyId: "", error: "" });
  useLayoutEffect(() => {
    scope.active = true;
    operationRef.current = null;
    return () => { scope.active = false; };
  }, [scope]);
  const operate = useCallback(async (mutation?: Mutation) => {
    if (!scope.active || currentScope.current !== scope || !scope.authority || operationRef.current) return;
    const operation = {}; operationRef.current = operation;
    const current = () => scope.active && currentScope.current === scope && operationRef.current === operation;
    const publish = (update: Partial<PinView>) => {
      if (current()) setView((previous) => ({ scope, items: [], loading: false, busyId: "", error: "", ...(previous.scope === scope ? previous : {}), ...update }));
    };
    publish({ loading: !mutation, busyId: mutation?.eventId ?? "", error: "" });
    try {
      const request = { roomId, channelId, authority: scope.authority, beforeDispatch: () => {
        if (!current()) throw new Error("메시지 고정 요청의 방 또는 권위가 바뀌었어요.");
      } };
      const items = mutation ? await setMessagePinned({ ...request, ...mutation }) : await fetchMessagePins(request);
      publish({ items });
    } catch (cause) {
      publish({ error: cause instanceof Error ? cause.message : "고정 메시지를 불러오거나 바꾸지 못했어요." });
    } finally {
      if (current()) { publish({ loading: false, busyId: "" }); operationRef.current = null; }
    }
  }, [channelId, roomId, scope]);
  const setPinsError = useCallback((error: string) => {
    if (scope.active && currentScope.current === scope) setView((previous) => ({ scope, items: [], loading: false, busyId: "", ...(previous.scope === scope ? previous : {}), error }));
  }, [scope]);
  const reloadPins = useCallback(() => operate(), [operate]);
  const setPinned = useCallback((eventId: string, pinned: boolean) => operate({ eventId, pinned }), [operate]);
  const visible = view.scope === scope ? view : null;
  const pinBusyIds = useMemo(() => new Set(visible?.busyId ? [visible.busyId] : []), [visible?.busyId]);
  return { pinnedItems: visible?.items ?? [], pinsLoading: visible?.loading ?? false,
    pinsError: visible?.error ?? "", pinBusyIds, reloadPins, setPinned, setPinsError };
}
