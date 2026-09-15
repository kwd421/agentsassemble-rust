import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "9af8355cbdcdedbc9848e05361a37a34b43df74230331e6de1e2751faacc7681",
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
