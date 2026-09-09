import type { MouseEvent as ReactMouseEvent } from "react";
import { Activity, Check, LogOut, Plus, Settings, UserPlus, Users } from "lucide-react";
import {
  completeRoomAppearance,
  roomAppearanceStyle,
  type RoomAppearance,
} from "../../lib/roomAppearance";
import {
  roomSettingsKey,
  roomIsDisconnected,
  type RoomDockItem,
} from "../../lib/roomDockModel";
import {
  ROOM_RAIL_MENU_SIZE,
  ROOM_RAIL_MENU_VIEWPORT_MARGIN,
} from "../../lib/roomRailMenuPosition";

export type RoomMenuState = {
  roomId: string;
  x: number;
  y: number;
} | null;

export const MOBILE_ROOM_RAIL_WIDTH = 68;

export default function RoomRail({
  rooms,
  activeRoom,
  roomAppearances,
  guestLocked,
  canManageActiveRoom = false,
  adminOpen,
  menuRoom,
  roomMenu,
  mobileViewport = false,
  inert = false,
  onSelectRoom,
  onOpenAdmin, onAddRoom, onManageRooms, onOpenFriends, friendsOpen = false,
  onOpenRoomMenu,
  onMarkRoomRead,
  readReady = false,
  readStatus = "loading",
  readError = "",
  onRetryRoomRead,
  onInviteRoom,
  onOpenRoomSettings,
  onLeaveRoom,
}: {
  rooms: RoomDockItem[];
  activeRoom: RoomDockItem;
  roomAppearances: Record<string, RoomAppearance>;
  guestLocked: boolean;
  canManageActiveRoom?: boolean;
  adminOpen: boolean;
  menuRoom?: RoomDockItem;
  roomMenu: RoomMenuState;
  mobileViewport?: boolean;
  inert?: boolean;
  onSelectRoom: (roomId: string) => void;
  onAddRoom: () => void;
  onManageRooms?: () => void;
  onOpenAdmin?: () => void;
  onOpenFriends?: () => void;
  friendsOpen?: boolean;
  onOpenRoomMenu: (event: ReactMouseEvent, room: RoomDockItem) => void;
  onMarkRoomRead: (roomId: string) => void | Promise<void>;
  readReady?: boolean;
  readStatus?: "loading" | "ready" | "saving" | "stale" | "error";
  readError?: string;
  onRetryRoomRead?: () => void;
  onInviteRoom: (roomId: string) => void;
  onOpenRoomSettings: (roomId: string) => void;
  onLeaveRoom: (roomId: string) => void;
}) {
  const buttonStyle = mobileViewport ? { width: 44, height: 44, flexBasis: 44 } : undefined;
  return (
    <nav
      className="dc-rail flex shrink-0 flex-col items-center gap-2 py-3"
      aria-label="룸 레일"
      inert={inert}
      style={mobileViewport ? { width: MOBILE_ROOM_RAIL_WIDTH } : undefined}
    >
      {onOpenFriends && <button type="button" className="dc-server-btn" style={buttonStyle} aria-label="친구" title="친구" aria-pressed={friendsOpen} data-active={friendsOpen} onClick={onOpenFriends}><Users size={20} /></button>}
      <div className="dc-room-stack min-h-0 flex-1 overflow-y-auto chat-scroll" aria-label="방 목록">
        {rooms.map((room) => {
          const Icon = room.icon;
          const active = !adminOpen && !friendsOpen && activeRoom.id === room.id;
          const disconnected = roomIsDisconnected(room);
          const roomAppearance = completeRoomAppearance(
            {
              ...room.appearance,
              ...(roomAppearances[roomSettingsKey(room)] || roomAppearances[room.id]),
            }
          );
          return (
            <button
              key={room.id}
              type="button"
              onClick={() => onSelectRoom(room.id)}
              onContextMenu={(event) => onOpenRoomMenu(event, room)}
              data-active={active}
              data-tone={room.tone}
              data-has-image={Boolean(roomAppearance.iconImage)}
              data-connection-state={disconnected ? "disconnected" : room.connectionState || "local"}
              style={{ ...roomAppearanceStyle(roomAppearance), ...buttonStyle }}
              className="dc-server-btn"
              aria-label={`${room.label}${disconnected ? " · 연결이 끊긴 서버" : ""}`}
              title={`${room.label} · ${disconnected ? "연결이 끊긴 서버" : room.topic}`}
            >
              {roomAppearance.iconImage ? null : <Icon size={18} aria-hidden />}
              {disconnected && <span className="dc-server-connection-dot" aria-hidden />}
              <span className="sr-only">{room.shortLabel}</span>
            </button>
          );
        })}
        {!guestLocked && (
          <button
            type="button"
            onClick={onAddRoom}
            className="dc-server-btn dc-server-add"
            aria-label="새 방 만들기"
            title="새 방"
            style={buttonStyle}
          >
            <Plus size={20} />
          </button>
        )}
      </div>
      {onOpenAdmin && <button type="button" className="dc-server-btn" style={buttonStyle} aria-label="서버 상태" title="서버 상태" aria-pressed={adminOpen} onClick={onOpenAdmin}><Activity size={20} /></button>}
      {onManageRooms && <button type="button" className="dc-server-btn" style={{ marginBottom: 80, ...buttonStyle }} aria-label="방 관리" title="방 관리" onClick={onManageRooms}><Settings size={20} /></button>}
      {menuRoom && roomMenu && (
        <div
          className="dc-context-menu"
          style={{
            left: roomMenu.x,
            top: roomMenu.y,
            width: ROOM_RAIL_MENU_SIZE.width,
            maxHeight: `calc(100vh - ${roomMenu.y}px - ${ROOM_RAIL_MENU_VIEWPORT_MARGIN}px)`,
          }}
          role="menu"
          aria-label={`${menuRoom.label} 서버 메뉴`}
          onClick={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.preventDefault()}
        >
          <button type="button" role="menuitem" disabled={!readReady} onClick={() => void onMarkRoomRead(menuRoom.id)}>
            <Check size={16} />
            {readStatus === "saving" ? "읽음 저장 중…" : "읽음으로 표시하기"}
          </button>
          {readError && <p role="alert" className="preserve-words">{readError}</p>}
          {(readStatus === "stale" || readStatus === "error") && onRetryRoomRead && (
            <button type="button" role="menuitem" onClick={onRetryRoomRead}>읽음 설정 다시 불러오기</button>
          )}
          {!guestLocked && menuRoom && !roomIsDisconnected(menuRoom) && (
            <button type="button" role="menuitem" onClick={() => onInviteRoom(menuRoom.id)}>
              <UserPlus size={16} />
              서버에 초대하기
            </button>
          )}
          {(!guestLocked || (canManageActiveRoom && menuRoom.id === activeRoom.id)) && !roomIsDisconnected(menuRoom) && (
            <button type="button" role="menuitem" onClick={() => onOpenRoomSettings(menuRoom.id)}>
              <Settings size={16} />
              서버 설정
            </button>
          )}
          {guestLocked && (
            <>
              <span className="dc-context-separator" aria-hidden />
              <button type="button" role="menuitem" className="danger" onClick={() => onLeaveRoom(menuRoom.id)}>
                <LogOut size={16} />
                서버 나가기
              </button>
            </>
          )}
        </div>
      )}
      <div className="mt-auto" />
    </nav>
  );
}
