type TauriInternals = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
};

function tauriInternals(): TauriInternals | undefined {
  return (
    window as typeof window & {
      __TAURI_INTERNALS__?: TauriInternals;
    }
  ).__TAURI_INTERNALS__;
}

export function isDesktopWebview(): boolean {
  return Boolean(tauriInternals());
}

export interface DesktopRuntimeTicket {
  ticket: string;
  ttl_seconds: number;
  websocket_base_url: string;
  server_proof_key: string;
}

export interface DesktopWorkspaceSelection {
  selected: boolean;
  path: string;
}

export async function requestDesktopRuntimeTicket(
  roomId: string
): Promise<DesktopRuntimeTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  return tauri.invoke<DesktopRuntimeTicket>("runtime_ticket", { roomId });
}

export async function chooseDesktopWorkspace(): Promise<DesktopWorkspaceSelection> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("workspace_picker_unavailable");
  }
  return tauri.invoke<DesktopWorkspaceSelection>("choose_local_workspace");
}

export async function openDesktopCentralGoogleLogin(url: string): Promise<void> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 중앙 로그인 기능을 사용할 수 없습니다.");
  }
  await tauri.invoke("open_central_google_login", { url });
}

export async function cacheNativeRoomDirectory(rooms: unknown[]): Promise<void> {
  const tauri = tauriInternals();
  if (!tauri) return;
  await tauri.invoke("cache_selected_room_directory", {
    rooms: JSON.stringify(rooms),
  });
}
