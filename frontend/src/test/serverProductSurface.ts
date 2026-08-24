import type { ServerProductSurface } from "../types/generated/ServerProductSurface";

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: 1,
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
    "room.random.choose",
    "room.random.roll",
    "room.settings.update",
  ],
};
