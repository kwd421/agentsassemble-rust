import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "d0105e5e16ad30d88da860d57ba6cb69edfd643e4886d60055e9e7dd4d995a36",
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
