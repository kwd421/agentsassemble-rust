import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "bbffa3e67de5df6d7c2553f4f48eb63f21dccc449168a7af6c7e2cf7083e6477",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...ROOM_ACTIONS],
};
