import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "6be8b1f11a4216be48651acb00745fba6b1eb16b6a116d8a676255ca38c25b27",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...ROOM_ACTIONS],
};
