import { useEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import {
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
  loadSidebarWidth,
  normalizeSidebarWidth,
  persistSidebarWidth,
  resizedSidebarWidth,
} from "../lib/sidebarResizeModel";

type SidebarResizeState = {
  startWidth: number;
  startX: number;
  currentWidth: number;
};

export function useSidebarResize() {
  const [channelSidebarWidth, setChannelSidebarWidth] = useState(loadSidebarWidth);
  const sidebarResizeRef = useRef<SidebarResizeState | null>(null);

  function startSidebarResize(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    const startWidth = normalizeSidebarWidth(channelSidebarWidth);
    sidebarResizeRef.current = {
      startWidth,
      startX: event.clientX,
      currentWidth: startWidth,
    };
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // Synthetic browser checks may not create a capturable native pointer.
    }
    document.body.dataset.sidebarResizing = "true";
  }

  function adjustSidebarWidthWithKeyboard(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (
      event.key !== "ArrowLeft" &&
      event.key !== "ArrowRight" &&
      event.key !== "Home" &&
      event.key !== "End"
    ) {
      return;
    }
    event.preventDefault();
    setChannelSidebarWidth((previous) => {
      const next =
        event.key === "Home"
          ? SIDEBAR_WIDTH_MIN
          : event.key === "End"
            ? SIDEBAR_WIDTH_MAX
            : normalizeSidebarWidth(previous + (event.key === "ArrowLeft" ? -16 : 16));
      persistSidebarWidth(next);
      return next;
    });
  }

  useEffect(() => {
    function handlePointerMove(event: PointerEvent) {
      const resize = sidebarResizeRef.current;
      if (!resize) return;
      const nextWidth = resizedSidebarWidth({
        startWidth: resize.startWidth,
        startX: resize.startX,
        currentX: event.clientX,
      });
      resize.currentWidth = nextWidth;
      setChannelSidebarWidth(nextWidth);
    }

    function finishSidebarResize() {
      const resize = sidebarResizeRef.current;
      if (!resize) return;
      sidebarResizeRef.current = null;
      delete document.body.dataset.sidebarResizing;
      const finalWidth = normalizeSidebarWidth(resize.currentWidth);
      persistSidebarWidth(finalWidth);
      setChannelSidebarWidth(finalWidth);
    }

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishSidebarResize);
    window.addEventListener("pointercancel", finishSidebarResize);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishSidebarResize);
      window.removeEventListener("pointercancel", finishSidebarResize);
      delete document.body.dataset.sidebarResizing;
    };
  }, []);

  return {
    channelSidebarWidth,
    startSidebarResize,
    adjustSidebarWidthWithKeyboard,
  };
}
