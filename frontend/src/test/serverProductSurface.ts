import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "fe78efe7f6722e3ceab1f967e59457616753e898fba4b0634b5e4bd3fd0ffc0e",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...ROOM_ACTIONS],
};
