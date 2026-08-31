import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "12fab715f484d4c8057d88eba252a33a9f0c0372c50cd97c468e70475e0a3ba4",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...ROOM_ACTIONS],
};
