import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "18570cb82bfb42ce34db43d9e52dfaf28b07400780c8a82bc2b2e4422743cead",
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
