import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "eb570f772148126e7925852a6e9487e46044b71b2d0e455f825dc3838dc5dd9d",
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
