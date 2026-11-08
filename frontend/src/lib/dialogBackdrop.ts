import type { MouseEvent, PointerEvent } from "react";

const pressedOnBackdrop = new WeakSet<EventTarget>();

/**
 * Dismiss handlers for a modal `<dialog>`'s backdrop, where the dialog itself
 * is the event target. A click counts only when the press also started on the
 * backdrop: a text selection dragged out of a field and released over the
 * backdrop reports the dialog as the click target too, and must not close it.
 * `outsideBox` is for dialogs whose own padding is also the dialog element.
 */
export function backdropDismiss(onDismiss: () => void, { outsideBox = false, stopPropagation = false } = {}) {
  return {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (backdropHit(event, outsideBox)) pressedOnBackdrop.add(event.currentTarget);
      else pressedOnBackdrop.delete(event.currentTarget);
    },
    onClick(event: MouseEvent<HTMLElement>) {
      if (stopPropagation) event.stopPropagation();
      if (pressedOnBackdrop.delete(event.currentTarget) && backdropHit(event, outsideBox)) onDismiss();
    },
  };
}

function backdropHit(event: MouseEvent<HTMLElement>, outsideBox: boolean): boolean {
  if (event.target !== event.currentTarget) return false;
  if (!outsideBox) return true;
  const bounds = event.currentTarget.getBoundingClientRect();
  return event.clientX < bounds.left || event.clientX > bounds.right ||
    event.clientY < bounds.top || event.clientY > bounds.bottom;
}
