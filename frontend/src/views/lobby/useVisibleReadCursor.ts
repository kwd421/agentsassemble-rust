import { useEffect, useRef, type RefObject } from "react";
import { feedIsNearBottom } from "./useLobbyHistory";

const openModal = () => document.querySelector('dialog[open], [aria-modal="true"]:not(dialog)');

// Acknowledgement belongs to the existing preference owner. This hook only
// establishes that the active user is actually viewing the latest feed.
export function useVisibleReadCursor({
  roomId, enabled, latestSequence, lastReadSequence, scrollRef, onMarkRead,
}: {
  roomId: string;
  enabled: boolean;
  latestSequence: number;
  lastReadSequence: number;
  scrollRef: RefObject<HTMLDivElement | null>;
  onMarkRead?: (cursor?: string) => void;
}) {
  const attempted = useRef<{ roomId: string; sequence: number } | null>(null);
  useEffect(() => {
    if (attempted.current?.roomId === roomId && lastReadSequence >= attempted.current.sequence) {
      attempted.current = null;
    }
    if (!enabled || !onMarkRead || latestSequence <= lastReadSequence) return;
    let modalObserver: MutationObserver | null = null;
    const acknowledgeVisibleLatest = () => {
      const feed = scrollRef.current;
      if (!feed || feed.clientHeight <= 0 || document.visibilityState !== "visible" ||
          !document.hasFocus() ||
          !feedIsNearBottom(feed)) return;
      if (openModal()) {
        // Modal state is owned by the dialog/portal DOM. Subscribe only while it
        // blocks a visible read, then recheck all guards after its dismissal.
        if (!modalObserver) {
          modalObserver = new MutationObserver(() => {
            if (openModal()) return;
            modalObserver?.disconnect();
            modalObserver = null;
            acknowledgeVisibleLatest();
          });
          modalObserver.observe(document.body, {
            childList: true, subtree: true, attributes: true, attributeFilter: ["open", "aria-modal"],
          });
        }
        return;
      }
      modalObserver?.disconnect();
      modalObserver = null;
      const bounds = feed.getBoundingClientRect();
      const pointX = (bounds.left + bounds.right) / 2;
      const pointY = Math.min(bounds.bottom, window.innerHeight) - 1;
      const visibleTarget = document.elementFromPoint(pointX, pointY);
      if (!visibleTarget || !feed.contains(visibleTarget)) return;
      if (attempted.current?.roomId === roomId && attempted.current.sequence === latestSequence) return;
      attempted.current = { roomId, sequence: latestSequence };
      onMarkRead(`seq:${latestSequence}`);
    };
    acknowledgeVisibleLatest();
    const feed = scrollRef.current;
    feed?.addEventListener("scroll", acknowledgeVisibleLatest);
    window.addEventListener("focus", acknowledgeVisibleLatest);
    document.addEventListener("visibilitychange", acknowledgeVisibleLatest);
    return () => {
      modalObserver?.disconnect();
      feed?.removeEventListener("scroll", acknowledgeVisibleLatest);
      window.removeEventListener("focus", acknowledgeVisibleLatest);
      document.removeEventListener("visibilitychange", acknowledgeVisibleLatest);
    };
  }, [enabled, lastReadSequence, latestSequence, onMarkRead, roomId, scrollRef]);
}
