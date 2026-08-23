import { useEffect, type Dispatch, type SetStateAction } from "react";
import type { ChannelMenuState } from "./appModel";
import type { RoomMenuState } from "../views/components/RoomRail";

export function useDismissMenus(
  roomMenu: RoomMenuState,
  channelMenu: ChannelMenuState,
  setRoomMenu: Dispatch<SetStateAction<RoomMenuState>>,
  setChannelMenu: Dispatch<SetStateAction<ChannelMenuState>>
) {
  useEffect(() => {
    if (!roomMenu && !channelMenu) return;
    function closeMenu() {
      setRoomMenu(null);
      setChannelMenu(null);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") closeMenu();
    }
    window.addEventListener("click", closeMenu);
    window.addEventListener("resize", closeMenu);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("click", closeMenu);
      window.removeEventListener("resize", closeMenu);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [roomMenu, channelMenu, setRoomMenu, setChannelMenu]);
}
