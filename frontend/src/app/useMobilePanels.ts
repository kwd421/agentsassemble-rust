import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { MobileRoomInfoInitialMode } from "./appModel";

type MobilePanelDragState = {
  startX: number;
  startY: number;
  sidebarOpen: boolean;
};

const MOBILE_SWIPE_THRESHOLD = 42;
const MOBILE_SWIPE_VERTICAL_TOLERANCE = 80;

export function mobileViewportMatches() {
  return (
    typeof window !== "undefined" &&
    window.matchMedia?.("(max-width: 760px)").matches
  );
}

export function useMobilePanels({ canOpenRoomInfo }: { canOpenRoomInfo: boolean }) {
  const [mobileSidebarOpen, setMobileSidebarOpen] = useState(mobileViewportMatches);
  const [mobileRoomInfoOpen, setMobileRoomInfoOpen] = useState(false);
  const [mobileRoomInfoInitialMode, setMobileRoomInfoInitialMode] =
    useState<MobileRoomInfoInitialMode>("info");
  const mobilePanelDragRef = useRef<MobilePanelDragState | null>(null);

  function mobileViewportIsActive() {
    return mobileViewportMatches();
  }

  function openMobileSidebar() {
    setMobileRoomInfoOpen(false);
    setMobileSidebarOpen(true);
  }

  function closeMobileSidebar() {
    setMobileSidebarOpen(false);
  }

  function openMobileRoomInfo() {
    if (!canOpenRoomInfo) return;
    setMobileSidebarOpen(false);
    setMobileRoomInfoInitialMode("info");
    setMobileRoomInfoOpen(true);
  }

  function closeMobileRoomInfo() {
    setMobileRoomInfoOpen(false);
    setMobileRoomInfoInitialMode("info");
  }

  function openMobileProfileFromPanel() {
    document.querySelector<HTMLElement>(".dc-sidebar .dc-user-identity")?.click();
  }

  function closeMobileOverlays() {
    setMobileSidebarOpen(false);
    setMobileRoomInfoOpen(false);
  }

  function mobileGestureCanStart(target: HTMLElement | null, sidebarOpen: boolean) {
    const blockedSelector = sidebarOpen
      ? "input, textarea, select, a, [role='dialog']"
      : "button, input, textarea, select, a, [role='dialog']";
    return !target?.closest(blockedSelector);
  }

  function finishMobilePanelGesture(currentX: number, currentY: number) {
    const drag = mobilePanelDragRef.current;
    if (!drag) return;
    mobilePanelDragRef.current = null;
    if (!mobileViewportIsActive()) return;
    const deltaX = currentX - drag.startX;
    const deltaY = Math.abs(currentY - drag.startY);
    if (
      deltaY > MOBILE_SWIPE_VERTICAL_TOLERANCE ||
      Math.abs(deltaX) < MOBILE_SWIPE_THRESHOLD
    ) {
      return;
    }
    if (drag.sidebarOpen && deltaX < -MOBILE_SWIPE_THRESHOLD) {
      closeMobileSidebar();
      return;
    }
    if (!drag.sidebarOpen && deltaX > MOBILE_SWIPE_THRESHOLD) {
      openMobileSidebar();
    }
  }

  function handleMobileShellPointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (!mobileViewportIsActive() || mobileRoomInfoOpen) return;
    const target = event.target as HTMLElement | null;
    if (!mobileGestureCanStart(target, mobileSidebarOpen)) return;
    mobilePanelDragRef.current = {
      startX: event.clientX,
      startY: event.clientY,
      sidebarOpen: mobileSidebarOpen,
    };
  }

  function handleMobileShellPointerEnd(event: ReactPointerEvent<HTMLDivElement>) {
    finishMobilePanelGesture(event.clientX, event.clientY);
  }

  function cancelMobileShellPointer() {
    mobilePanelDragRef.current = null;
  }

  useEffect(() => {
    if (!mobileSidebarOpen && !mobileRoomInfoOpen) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMobileOverlays();
    }
    function closeOnDesktopResize() {
      if (!mobileViewportIsActive()) closeMobileOverlays();
    }
    window.addEventListener("keydown", closeOnEscape);
    window.addEventListener("resize", closeOnDesktopResize);
    return () => {
      window.removeEventListener("keydown", closeOnEscape);
      window.removeEventListener("resize", closeOnDesktopResize);
    };
  }, [mobileRoomInfoOpen, mobileSidebarOpen]);

  useEffect(() => {
    function handleTouchStart(event: TouchEvent) {
      if (!mobileViewportIsActive() || mobileRoomInfoOpen || event.touches.length !== 1) return;
      const target = event.target as HTMLElement | null;
      if (!mobileGestureCanStart(target, mobileSidebarOpen)) return;
      const touch = event.touches[0];
      mobilePanelDragRef.current = {
        startX: touch.clientX,
        startY: touch.clientY,
        sidebarOpen: mobileSidebarOpen,
      };
    }

    function handleTouchEnd(event: TouchEvent) {
      const touch = event.changedTouches[0];
      if (touch) finishMobilePanelGesture(touch.clientX, touch.clientY);
    }

    function handleTouchMove(event: TouchEvent) {
      const drag = mobilePanelDragRef.current;
      const touch = event.touches[0];
      if (!drag || !touch) return;
      const deltaX = touch.clientX - drag.startX;
      const deltaY = Math.abs(touch.clientY - drag.startY);
      if (Math.abs(deltaX) > 12 && Math.abs(deltaX) > deltaY) event.preventDefault();
    }

    function handleTouchCancel() {
      mobilePanelDragRef.current = null;
    }

    window.addEventListener("touchstart", handleTouchStart, { passive: true });
    window.addEventListener("touchmove", handleTouchMove, { passive: false });
    window.addEventListener("touchend", handleTouchEnd, { passive: true });
    window.addEventListener("touchcancel", handleTouchCancel, { passive: true });
    return () => {
      window.removeEventListener("touchstart", handleTouchStart);
      window.removeEventListener("touchmove", handleTouchMove);
      window.removeEventListener("touchend", handleTouchEnd);
      window.removeEventListener("touchcancel", handleTouchCancel);
    };
  }, [mobileRoomInfoOpen, mobileSidebarOpen]);

  return {
    mobileSidebarOpen,
    setMobileSidebarOpen,
    mobileRoomInfoOpen,
    setMobileRoomInfoOpen,
    mobileRoomInfoInitialMode,
    setMobileRoomInfoInitialMode,
    mobileViewportIsActive,
    openMobileSidebar,
    closeMobileSidebar,
    openMobileRoomInfo,
    closeMobileRoomInfo,
    openMobileProfileFromPanel,
    closeMobileOverlays,
    handleMobileShellPointerDown,
    handleMobileShellPointerEnd,
    cancelMobileShellPointer,
  };
}
