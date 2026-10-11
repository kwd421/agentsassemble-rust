import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

const EDGE = 8;

// One styled hint for every control that names itself with `title` (or `data-tip`).
// The native title is parked while the hint is up, so the system tooltip does not appear as well.
export default function TooltipLayer() {
  const [tip, setTip] = useState<{ text: string; rect: DOMRect } | null>(null);
  const bubble = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let current: HTMLElement | null = null;
    let parked: string | null = null;
    const release = () => {
      if (current && parked !== null && !current.hasAttribute("title")) current.setAttribute("title", parked);
      current = null; parked = null;
      setTip(null);
    };
    const show = (target: EventTarget | null) => {
      const element = target instanceof Element ? target.closest<HTMLElement>("[data-tip], [title]") : null;
      if (element === current) return;
      release();
      const text = (element?.dataset.tip || element?.getAttribute("title") || "").trim();
      if (!element || !text) return;
      current = element;
      parked = element.getAttribute("title");
      element.removeAttribute("title");
      setTip({ text, rect: element.getBoundingClientRect() });
    };
    const over = (event: PointerEvent) => { if (event.pointerType !== "touch") show(event.target); };
    const focus = (event: FocusEvent) => {
      if (event.target instanceof HTMLElement && event.target.matches(":focus-visible")) show(event.target);
    };
    const move = () => { if (current && !current.isConnected) release(); };
    document.addEventListener("pointerover", over);
    document.addEventListener("pointermove", move);
    document.addEventListener("focusin", focus);
    document.addEventListener("focusout", release);
    document.addEventListener("pointerdown", release);
    document.addEventListener("keydown", release);
    document.addEventListener("scroll", release, true);
    document.documentElement.addEventListener("pointerleave", release);
    return () => {
      document.removeEventListener("pointerover", over);
      document.removeEventListener("pointermove", move);
      document.removeEventListener("focusin", focus);
      document.removeEventListener("focusout", release);
      document.removeEventListener("pointerdown", release);
      document.removeEventListener("keydown", release);
      document.removeEventListener("scroll", release, true);
      document.documentElement.removeEventListener("pointerleave", release);
      release();
    };
  }, []);
  useLayoutEffect(() => {
    const node = bubble.current;
    if (!tip || !node) return;
    const { width, height } = node.getBoundingClientRect();
    const center = tip.rect.left + tip.rect.width / 2;
    const left = Math.max(EDGE, Math.min(window.innerWidth - width - EDGE, center - width / 2));
    const above = tip.rect.top - height - EDGE >= EDGE;
    node.dataset.placement = above ? "top" : "bottom";
    node.style.left = `${left}px`;
    node.style.top = `${above ? tip.rect.top - height - EDGE : tip.rect.bottom + EDGE}px`;
    node.style.setProperty("--dc-tooltip-arrow", `${Math.max(12, Math.min(width - 12, center - left))}px`);
    node.style.visibility = "visible";
  }, [tip]);
  return tip ? createPortal(
    <div ref={bubble} className="dc-tooltip" role="tooltip" style={{ visibility: "hidden" }}>{tip.text}</div>,
    document.body,
  ) : null;
}
