export interface RoomRuntimeTicket {
  ticket: string;
  ttl_seconds: number;
  websocket_base_url: string;
  server_proof_key: string;
  displayResourceBase: string;
}

const SECRET_PATTERN = /^[0-9a-f]{64}$/;

function exactObject(
  value: unknown,
  keys: readonly string[]
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Room runtime ticket response is invalid.");
  }
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    throw new Error("Room runtime ticket response contract does not match.");
  }
  return record;
}

function ticketFields(
  ticket: unknown,
  ttlSeconds: unknown,
  serverProofKey: unknown
) {
  if (
    typeof ticket !== "string" ||
    !SECRET_PATTERN.test(ticket) ||
    !Number.isSafeInteger(ttlSeconds) ||
    Number(ttlSeconds) < 1 ||
    typeof serverProofKey !== "string" ||
    !SECRET_PATTERN.test(serverProofKey)
  ) {
    throw new Error("Room runtime ticket authority is invalid.");
  }
  return {
    ticket,
    ttl_seconds: ttlSeconds as number,
    server_proof_key: serverProofKey,
  };
}

function exactOrigin(value: string, protocols: readonly string[]): URL {
  let origin: URL;
  try {
    origin = new URL(value);
  } catch {
    throw new Error("Room runtime origin is invalid.");
  }
  if (
    !protocols.includes(origin.protocol) ||
    origin.username ||
    origin.password ||
    origin.pathname !== "/" ||
    origin.search ||
    origin.hash ||
    origin.origin !== value
  ) {
    throw new Error("Room runtime origin is invalid.");
  }
  return origin;
}

export function parseNativeRoomRuntimeTicket(value: unknown): RoomRuntimeTicket {
  const grant = exactObject(value, [
    "ticket",
    "ttl_seconds",
    "websocket_base_url",
    "server_proof_key",
  ]);
  const fields = ticketFields(
    grant.ticket,
    grant.ttl_seconds,
    grant.server_proof_key
  );
  if (typeof grant.websocket_base_url !== "string") {
    throw new Error("Room runtime origin is invalid.");
  }
  const socketOrigin = exactOrigin(grant.websocket_base_url, ["ws:"]);
  if (socketOrigin.hostname !== "127.0.0.1" || !socketOrigin.port) {
    throw new Error("Room runtime origin is not local.");
  }
  return {
    ...fields,
    websocket_base_url: socketOrigin.origin,
    displayResourceBase: `http://127.0.0.1:${socketOrigin.port}`,
  };
}

export function parseBrowserRoomRuntimeTicket(
  value: unknown,
  pageHref: string
): RoomRuntimeTicket {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Room session socket ticket response is invalid.");
  }
  const grant = value as Record<string, unknown>;
  const fields = ticketFields(
    grant.ticket,
    grant.ttl_seconds,
    grant.server_proof_key
  );
  const displayOrigin = exactOrigin(new URL(pageHref).origin, ["http:", "https:"]);
  const socketProtocol = displayOrigin.protocol === "https:" ? "wss:" : "ws:";
  return {
    ...fields,
    websocket_base_url: `${socketProtocol}//${displayOrigin.host}`,
    displayResourceBase: displayOrigin.origin,
  };
}

export function requireAcceptedRoomRuntimeTicket(
  value: unknown
): RoomRuntimeTicket {
  const accepted = exactObject(value, [
    "ticket",
    "ttl_seconds",
    "websocket_base_url",
    "server_proof_key",
    "displayResourceBase",
  ]);
  const fields = ticketFields(
    accepted.ticket,
    accepted.ttl_seconds,
    accepted.server_proof_key
  );
  if (
    typeof accepted.websocket_base_url !== "string" ||
    typeof accepted.displayResourceBase !== "string"
  ) {
    throw new Error("Room runtime origin is invalid.");
  }
  const socketOrigin = exactOrigin(accepted.websocket_base_url, ["ws:", "wss:"]);
  const displayOrigin = exactOrigin(accepted.displayResourceBase, ["http:", "https:"]);
  const expectedSocketProtocol =
    displayOrigin.protocol === "https:" ? "wss:" : "ws:";
  if (
    socketOrigin.protocol !== expectedSocketProtocol ||
    socketOrigin.host !== displayOrigin.host
  ) {
    throw new Error("Room runtime socket and display origins do not match.");
  }
  return {
    ...fields,
    websocket_base_url: socketOrigin.origin,
    displayResourceBase: displayOrigin.origin,
  };
}
