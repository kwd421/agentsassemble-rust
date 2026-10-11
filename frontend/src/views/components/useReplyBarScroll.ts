import { useLayoutEffect, type RefObject } from "react";

// The reply bar takes its height from the message list. When the reader was at
// the latest message, keep that message visible instead of sliding it under the bar.
export function useReplyBarScroll(scrollRef: RefObject<HTMLElement | null>, replyId: string | undefined) {
  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!replyId || !node) return;
    if (node.scrollHeight - node.scrollTop - node.clientHeight <= 48) node.scrollTop = node.scrollHeight;
  }, [scrollRef, replyId]);
}
