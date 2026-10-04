import type { RoomDockItem } from "./roomDockModel";
export type ServerConnectionState = "connected" | "connecting" | "disconnected" | "central-unconfirmed";
export const CONNECTION_LABELS: Record<ServerConnectionState, string> = {
  connected: "연결됨", connecting: "연결 중", disconnected: "연결 끊김", "central-unconfirmed": "연결 끊김 · 중앙 확인 불가",
};
export function projectRoomConnections(rooms: RoomDockItem[], localState: ServerConnectionState | null,
  centralUnavailable: boolean): RoomDockItem[] {
  return rooms.map(room => {
    if (room.roomOrigin !== "remote_server" && localState) {
      return { ...room, connectionState: localState === "connected" ? "local" : localState };
    }
    if (room.roomOrigin === "remote_server" && room.connectionState !== "connected" && centralUnavailable) {
      return { ...room, connectionState: "central-unconfirmed" };
    }
    return room;
  });
}
