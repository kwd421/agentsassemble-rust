import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { ROOM_ACTIONS } from "../types/generated/ROOM_ACTIONS";
import { ROOM_STREAMS } from "../types/generated/ROOM_STREAMS";
import { lengthDelimitedTranscript, sha256Hex } from "../lib/lengthDelimitedCrypto";

export function testServerProductSurfaceDigest(
  streams: readonly string[],
  actions: readonly string[] = ROOM_ACTIONS
): Promise<string> {
  return sha256Hex(
    lengthDelimitedTranscript("agentsassemble.server-product-surface.v1", [
      String(PRODUCT_SURFACE_REVISION),
      "streams", ...streams,
      "actions", ...actions,
    ])
  );
}

export const TEST_SERVER_PRODUCT_SURFACE: ServerProductSurface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: await testServerProductSurfaceDigest(ROOM_STREAMS),
  http_routes: [],
  websocket_streams: [...ROOM_STREAMS],
  websocket_actions: [...ROOM_ACTIONS],
};
