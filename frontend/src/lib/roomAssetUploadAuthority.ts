import type { DesktopManagerRoomAuthority } from "./desktopBridge";

export type RoomAssetUploadAuthority =
  | { kind: "local"; manager: DesktopManagerRoomAuthority }
  | { kind: "remote"; sessionToken: string; deviceToken: string };
