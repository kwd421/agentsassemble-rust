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

export interface DesktopBootstrapGrant {
  phase: "empty" | "initializing" | "complete" | "repair_required";
  authority_lineage_id: string;
  server_id: string;
  profile: {
    revision: number;
    display_name: string;
    handle: string;
    status: string;
    custom_status: string;
    avatar_label: string;
    avatar_image_url: string;
    banner_preset: string;
    accent_color: string;
    mic_muted: boolean;
    deafened: boolean;
    created_at: string;
    updated_at: string;
  } | null;
  deduplicated: boolean;
}

export interface DesktopOperatorHttpTicket {
  ticket: string;
  ttl_seconds: number;
  http_base_url: string;
}

export interface DesktopWorkspaceSelection {
  selected: boolean;
  path: string;
}

let desktopRuntimeHttpBase = "";

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function exactObject(
  value: unknown,
  keys: readonly string[],
  label: string
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 응답 형식이 올바르지 않습니다.`);
  }
  const object = value as Record<string, unknown>;
  const actual = Object.keys(object).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    throw new Error(`${label} 응답 계약이 일치하지 않습니다.`);
  }
  return object;
}

function validateDesktopBootstrapGrant(value: unknown): DesktopBootstrapGrant {
  const grant = exactObject(
    value,
    ["phase", "authority_lineage_id", "server_id", "profile", "deduplicated"],
    "데스크톱 bootstrap"
  );
  if (
    !new Set(["empty", "initializing", "complete", "repair_required"]).has(
      String(grant.phase)
    ) ||
    typeof grant.authority_lineage_id !== "string" ||
    !UUID_PATTERN.test(grant.authority_lineage_id) ||
    typeof grant.server_id !== "string" ||
    !UUID_PATTERN.test(grant.server_id) ||
    typeof grant.deduplicated !== "boolean"
  ) {
    throw new Error("데스크톱 bootstrap 권위 식별자가 올바르지 않습니다.");
  }
  if (grant.profile !== null) {
    const profile = exactObject(
      grant.profile,
      [
        "revision",
        "display_name",
        "handle",
        "status",
        "custom_status",
        "avatar_label",
        "avatar_image_url",
        "banner_preset",
        "accent_color",
        "mic_muted",
        "deafened",
        "created_at",
        "updated_at",
      ],
      "데스크톱 bootstrap profile"
    );
    if (
      !Number.isSafeInteger(profile.revision) ||
      Number(profile.revision) < 1 ||
      [
        "display_name",
        "handle",
        "status",
        "custom_status",
        "avatar_label",
        "avatar_image_url",
        "banner_preset",
        "accent_color",
        "created_at",
        "updated_at",
      ].some((key) => typeof profile[key] !== "string") ||
      typeof profile.mic_muted !== "boolean" ||
      typeof profile.deafened !== "boolean"
    ) {
      throw new Error("데스크톱 bootstrap profile이 올바르지 않습니다.");
    }
  }
  if ((grant.phase === "complete") !== (grant.profile !== null)) {
    throw new Error("데스크톱 bootstrap 단계와 profile이 일치하지 않습니다.");
  }
  return grant as unknown as DesktopBootstrapGrant;
}

function validatedDesktopHttpBase(value: string): string {
  const endpoint = new URL(value);
  if (
    endpoint.protocol !== "http:" ||
    endpoint.hostname !== "127.0.0.1" ||
    !endpoint.port ||
    endpoint.username ||
    endpoint.password ||
    endpoint.pathname !== "/" ||
    endpoint.search ||
    endpoint.hash
  ) {
    throw new Error("데스크톱 Rust 런타임 주소가 안전하지 않습니다.");
  }
  return `http://127.0.0.1:${endpoint.port}`;
}

function rememberDesktopRuntime(ticket: DesktopRuntimeTicket): DesktopRuntimeTicket {
  const endpoint = new URL(ticket.websocket_base_url);
  if (endpoint.protocol !== "ws:" || endpoint.hostname !== "127.0.0.1" || !endpoint.port) {
    throw new Error("데스크톱 Rust 런타임 주소가 안전하지 않습니다.");
  }
  desktopRuntimeHttpBase = `http://127.0.0.1:${endpoint.port}`;
  return ticket;
}

function rememberDesktopOperatorRuntime(
  ticket: DesktopOperatorHttpTicket
): DesktopOperatorHttpTicket {
  desktopRuntimeHttpBase = validatedDesktopHttpBase(ticket.http_base_url);
  return ticket;
}

export async function requestDesktopRuntimeTicket(
  roomId: string
): Promise<DesktopRuntimeTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  return tauri
    .invoke<DesktopRuntimeTicket>("runtime_ticket", { roomId })
    .then(rememberDesktopRuntime);
}

export async function requestDesktopBootstrapStatus(): Promise<DesktopBootstrapGrant> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  return tauri
    .invoke<unknown>("runtime_bootstrap_status")
    .then(validateDesktopBootstrapGrant);
}

export async function initializeDesktopBootstrap(
  requestId: string,
  displayName: string
): Promise<DesktopBootstrapGrant> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  return tauri
    .invoke<unknown>("runtime_bootstrap_initialize", {
      requestId,
      displayName,
    })
    .then(validateDesktopBootstrapGrant);
}

export async function requestDesktopOperatorTicket(): Promise<DesktopOperatorHttpTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  return tauri
    .invoke<DesktopOperatorHttpTicket>("runtime_operator_ticket")
    .then(rememberDesktopOperatorRuntime);
}

export async function fetchDesktopRuntime(
  roomId: string,
  path: string,
  init: RequestInit = {}
): Promise<Response> {
  if (!path.startsWith("/") || path.startsWith("//")) {
    throw new Error("데스크톱 Rust 런타임 경로가 잘못되었습니다.");
  }
  const issued = await requestDesktopRuntimeTicket(roomId);
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${issued.ticket}`);
  return fetch(`${desktopRuntimeHttpBase}${path}`, { ...init, headers });
}

export async function fetchDesktopOperatorRuntime(
  path: string,
  init: RequestInit = {}
): Promise<Response> {
  if (!path.startsWith("/") || path.startsWith("//")) {
    throw new Error("데스크톱 Rust 런타임 경로가 잘못되었습니다.");
  }
  const issued = await requestDesktopOperatorTicket();
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${issued.ticket}`);
  return fetch(`${desktopRuntimeHttpBase}${path}`, { ...init, headers });
}

export function resolveDesktopRuntimeResource(value: string | undefined): string | undefined {
  if (!value || !value.startsWith("/api/attachments/")) return value;
  return desktopRuntimeHttpBase ? `${desktopRuntimeHttpBase}${value}` : value;
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
