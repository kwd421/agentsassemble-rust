// The workspace boundary owns these listeners. Credentials stay in memory and
// are never published in DOM events; a response only affects its exact session.
const rejections = new Map<string, Set<() => void>>();

export function observeOwnerSessionRejection(sessionToken: string, rejected: () => void) {
  const listeners = rejections.get(sessionToken) ?? new Set<() => void>();
  listeners.add(rejected);
  rejections.set(sessionToken, listeners);
  return () => {
    listeners.delete(rejected);
    if (!listeners.size) rejections.delete(sessionToken);
  };
}

export async function fetchOwnerSession(sessionToken: string | undefined, url: string, init: RequestInit): Promise<Response> {
  const response = await fetch(url, init);
  if (sessionToken && [401, 403].includes(response.status)) {
    rejections.get(sessionToken)?.forEach(rejected => rejected());
  }
  return response;
}
