import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "2c8637e4833be3dcf882019333ce2354b1f9dd8dfee7019f9792ab6c545b9f0b",
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
