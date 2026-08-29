import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "063c7b2b4c91703915879b7e6fc2f5efc2138272945207ff42dfdf5043020b78",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [
    "agent.configure",
    "agent.create",
    "agent.resume",
    "agent.start",
    "agent.stop",
    "message.send",
    "participant.leave",
    "participant.mute",
    "participant.role.update",
    "room.random.choose",
    "room.random.roll",
    "room.settings.update",
  ],
};
