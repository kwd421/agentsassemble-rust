import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "c".repeat(64),
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
