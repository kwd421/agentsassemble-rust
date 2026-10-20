import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, PointerEvent as ReactPointerEvent } from "react";

const STORAGE_KEY = "agentsassemble.rightPanel.width.v1";
export const RIGHT_PANEL_WIDTH_DEFAULT = 264;
export const RIGHT_PANEL_WIDTH_MIN = 240;
export const RIGHT_PANEL_WIDTH_MAX = 480;

export function normalizeRightPanelWidth(value: unknown): number {
  const width = value === null || value === undefined || value === "" ? NaN : Number(value);
  if (!Number.isFinite(width)) return RIGHT_PANEL_WIDTH_DEFAULT;
  return Math.min(RIGHT_PANEL_WIDTH_MAX, Math.max(RIGHT_PANEL_WIDTH_MIN, Math.round(width)));
}

function load(): number {
  try { return normalizeRightPanelWidth(window.localStorage.getItem(STORAGE_KEY)); }
  catch { return RIGHT_PANEL_WIDTH_DEFAULT; }
}

function persist(width: number) {
  // Best-effort UI preference; room state does not depend on local storage.
  try { window.localStorage.setItem(STORAGE_KEY, String(width)); } catch { /* unavailable storage */ }
}

/** Width of the right panel, dragged from its left edge: moving left makes it wider. */
export function useRightPanelResize() {
  const [width, setWidth] = useState(load);
  const drag = useRef<{ startWidth: number; startX: number; current: number } | null>(null);

  function start(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    drag.current = { startWidth: width, startX: event.clientX, current: width };
    try { event.currentTarget.setPointerCapture?.(event.pointerId); }
    catch { /* Synthetic browser checks may not create a capturable native pointer. */ }
    document.body.dataset.sidebarResizing = "true";
  }

  function adjustWithKeyboard(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    setWidth((previous) => {
      const next = event.key === "Home" ? RIGHT_PANEL_WIDTH_MIN : event.key === "End" ? RIGHT_PANEL_WIDTH_MAX
        : normalizeRightPanelWidth(previous + (event.key === "ArrowLeft" ? 16 : -16));
      persist(next);
      return next;
    });
  }

  useEffect(() => {
    function move(event: PointerEvent) {
      const state = drag.current;
      if (!state) return;
      state.current = normalizeRightPanelWidth(state.startWidth - (event.clientX - state.startX));
      setWidth(state.current);
    }
    function finish() {
      const state = drag.current;
      if (!state) return;
      drag.current = null;
      delete document.body.dataset.sidebarResizing;
      persist(state.current);
    }
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
    };
  }, []);

  return { width, start, adjustWithKeyboard };
}
