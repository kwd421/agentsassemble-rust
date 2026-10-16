import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

const EDGE = 8;

function clipped(element: HTMLElement) {
  return [element, ...element.querySelectorAll<HTMLElement>("*")].slice(0, 24)
    .some((node) => node.scrollWidth > node.clientWidth + 1);
}

// One styled hint for every control that names itself with `title` (or `data-tip`).
// The native title is parked while the hint is up, so the system tooltip does not appear as well.
export default function TooltipLayer() {
  const [tip, setTip] = useState<{ text: string; rect: DOMRect; side?: string } | null>(null);
  const bubble = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let current: HTMLElement | null = null;
    let parked: string | null = null;
    let borrowedTip = false;
    let shown = "";
    let orphaned = 0;
    const restore = () => {
      if (current && parked !== null && !current.hasAttribute("title")) current.setAttribute("title", parked);
      if (current && borrowedTip) delete current.dataset.tip;
      current = null; parked = null; borrowedTip = false;
    };
    const release = () => {
      window.clearTimeout(orphaned);
      restore();
      shown = "";
      setTip(null);
    };
    const show = (target: EventTarget | null) => {
      const element = target instanceof Element ? target.closest<HTMLElement>("[data-tip], [title]") : null;
      if (element === current) return;
      const text = (element?.dataset.tip || element?.getAttribute("title") || "").trim();
      if (!element || !text) { release(); return; }
      window.clearTimeout(orphaned);
      restore();
      current = element;
      // The native title stays parked as data-tip too, so the parked trigger is still
      // found by the selector on inner pointer moves instead of releasing the hint.
      parked = element.getAttribute("title");
      if (parked !== null) {
        borrowedTip = !element.dataset.tip;
        element.removeAttribute("title");
        if (borrowedTip) element.dataset.tip = parked;
      }
      // A control that already shows this text in full needs no hint.
      const wanted = !(element.textContent || "").includes(text) || clipped(element);
      // A re-rendered control with the same hint keeps the one already on screen.
      if (wanted && text === shown) return;
      shown = wanted ? text : "";
      setTip(wanted ? { text, rect: element.getBoundingClientRect(), side: element.closest<HTMLElement>("[data-tip-side]")?.dataset.tipSide } : null);
    };
    const over = (event: PointerEvent) => { if (event.pointerType !== "touch") show(event.target); };
    const focus = (event: FocusEvent) => {
      if (event.target instanceof HTMLElement && event.target.matches(":focus-visible")) show(event.target);
    };
    // A control swapped out under the pointer is usually replaced at once; wait for that before hiding.
    const move = () => {
      if (!current || current.isConnected) return;
      current = null; parked = null;
      window.clearTimeout(orphaned);
      orphaned = window.setTimeout(release, 150);
    };
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
    if (tip.side === "right") {
      const middle = tip.rect.top + tip.rect.height / 2;
      const top = Math.max(EDGE, Math.min(window.innerHeight - height - EDGE, middle - height / 2));
      node.dataset.placement = "right";
      node.style.left = `${tip.rect.right + EDGE + 4}px`;
      node.style.top = `${top}px`;
      node.style.setProperty("--dc-tooltip-arrow", `${Math.max(12, Math.min(height - 12, middle - top))}px`);
    } else {
      const center = tip.rect.left + tip.rect.width / 2;
      const left = Math.max(EDGE, Math.min(window.innerWidth - width - EDGE, center - width / 2));
      const above = tip.rect.top - height - EDGE >= EDGE;
      node.dataset.placement = above ? "top" : "bottom";
      node.style.left = `${left}px`;
      node.style.top = `${above ? tip.rect.top - height - EDGE : tip.rect.bottom + EDGE}px`;
      node.style.setProperty("--dc-tooltip-arrow", `${Math.max(12, Math.min(width - 12, center - left))}px`);
    }
    node.style.visibility = "visible";
  }, [tip]);
  return tip ? createPortal(
    <div ref={bubble} className="dc-tooltip" role="tooltip" style={{ visibility: "hidden" }}>{tip.text}</div>,
    document.body,
  ) : null;
}
