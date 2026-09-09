import { useEffect, useRef, type RefObject } from "react";

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
    const acknowledgeVisibleLatest = () => {
      const feed = scrollRef.current;
      if (!feed || feed.clientHeight <= 0 || document.visibilityState !== "visible" ||
          !document.hasFocus() || document.querySelector('[aria-modal="true"]') ||
          feed.scrollHeight - feed.scrollTop - feed.clientHeight > 2) return;
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
      feed?.removeEventListener("scroll", acknowledgeVisibleLatest);
      window.removeEventListener("focus", acknowledgeVisibleLatest);
      document.removeEventListener("visibilitychange", acknowledgeVisibleLatest);
    };
  }, [enabled, lastReadSequence, latestSequence, onMarkRead, roomId, scrollRef]);
}
