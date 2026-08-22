export interface DesktopTicketGrant {
  ticket: string;
  ttl_seconds: number;
  websocket_base_url: string;
  server_proof_key: string;
}

export function isDesktopShell(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function requestDesktopTicket(roomId: string): Promise<DesktopTicketGrant> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<DesktopTicketGrant>("runtime_ticket", { roomId });
}
