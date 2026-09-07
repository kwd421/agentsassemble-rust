import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "d3c4cfec40284b14176b509ef00b0868d3066f66d325bb26310d861dfb7b382f",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...ROOM_ACTIONS],
};
