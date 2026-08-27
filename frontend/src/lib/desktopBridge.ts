import type { HostProductSurface } from "../types/generated/HostProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

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
  server_product_surface_revision: number;
  server_product_surface_digest: string;
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

type DesktopHttpTicketCommand =
  | "runtime_preferences_read_ticket"
  | "runtime_preferences_write_ticket"
  | "runtime_human_invite_create_ticket"
  | "runtime_human_invite_revoke_ticket"
  | "runtime_settings_directory_read_ticket";

export interface DesktopCentralRegistrationBinding {
  server_id: string;
  host_public_key_x: string;
  host_key_fingerprint: string;
}

export interface DesktopCentralRegistrationTicket
  extends DesktopOperatorHttpTicket,
    DesktopCentralRegistrationBinding {}

export interface DesktopCentralRegistrationResponse {
  response: Response;
  binding: DesktopCentralRegistrationBinding;
}

export interface DesktopWorkspaceSelection {
  selected: boolean;
  path: string;
}

let desktopRuntimeHttpBase = "";
let desktopHostSurface: HostProductSurface | null = null;

function requireDesktopHostCommand(command: string) {
  if (!desktopHostSurface) {
    throw new Error("데스크톱 호스트 제품 표면이 아직 고정되지 않았습니다.");
  }
  if (!desktopHostSurface.commands.includes(command)) {
    throw new Error(`데스크톱 호스트 명령 ${command}은 현재 제품 표면에 없습니다.`);
  }
}

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

function validateHostProductSurface(value: unknown): HostProductSurface {
  const surface = exactObject(value, ["revision", "digest", "commands"], "호스트 제품 표면");
  if (
    surface.revision !== PRODUCT_SURFACE_REVISION ||
    !/^[0-9a-f]{64}$/.test(String(surface.digest)) ||
    !Array.isArray(surface.commands)
  ) {
    throw new Error("호스트 제품 표면이 올바르지 않습니다.");
  }
  const commands = surface.commands;
  if (commands.some((command) => typeof command !== "string")) {
    throw new Error("호스트 제품 표면 명령 등록부가 올바르지 않습니다.");
  }
  const sorted = [...commands].sort();
  if (
    sorted.length !== new Set(sorted).size ||
    sorted.some((command, index) => command !== commands[index])
  ) {
    throw new Error("호스트 제품 표면 명령 등록부가 올바르지 않습니다.");
  }
  return surface as unknown as HostProductSurface;
}

export async function requestDesktopHostProductSurface(): Promise<HostProductSurface> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 호스트 제품 표면을 사용할 수 없습니다.");
  }
  const surface = validateHostProductSurface(
    await tauri.invoke<unknown>("host_product_surface")
  );
  if (desktopHostSurface && desktopHostSurface.digest !== surface.digest) {
    throw new Error("데스크톱 호스트 제품 표면이 실행 중 변경되었습니다.");
  }
  desktopHostSurface = surface;
  return structuredClone(surface);
}

function validateDesktopBootstrapGrant(value: unknown): DesktopBootstrapGrant {
  const grant = exactObject(
    value,
    [
      "phase",
      "authority_lineage_id",
      "server_id",
      "server_product_surface_revision",
      "server_product_surface_digest",
      "profile",
      "deduplicated",
    ],
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
    grant.server_product_surface_revision !== PRODUCT_SURFACE_REVISION ||
    !/^[0-9a-f]{64}$/.test(String(grant.server_product_surface_digest)) ||
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
  const expected = `http://127.0.0.1:${endpoint.port}`;
  if (value !== expected) {
    throw new Error("데스크톱 Rust 런타임 주소가 정규 형식이 아닙니다.");
  }
  return expected;
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

function validateDesktopHttpTicket(
  value: unknown,
  label: string
): DesktopOperatorHttpTicket {
  const grant = exactObject(
    value,
    ["ticket", "ttl_seconds", "http_base_url"],
    label
  );
  if (
    typeof grant.ticket !== "string" ||
    !/^[0-9a-f]{64}$/.test(grant.ticket) ||
    !Number.isSafeInteger(grant.ttl_seconds) ||
    Number(grant.ttl_seconds) < 1 ||
    typeof grant.http_base_url !== "string"
  ) {
    throw new Error(`${label} 권위가 올바르지 않습니다.`);
  }
  return {
    ticket: grant.ticket,
    ttl_seconds: grant.ttl_seconds as number,
    http_base_url: validatedDesktopHttpBase(grant.http_base_url),
  };
}

async function requestDesktopHttpTicket(
  command: DesktopHttpTicketCommand,
  args: Record<string, unknown> | undefined,
  label: string
): Promise<DesktopOperatorHttpTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  requireDesktopHostCommand(command);
  return validateDesktopHttpTicket(
    await tauri.invoke<unknown>(command, args),
    label
  );
}

function validateDesktopCentralRegistrationTicket(
  value: unknown
): DesktopCentralRegistrationTicket {
  const grant = exactObject(
    value,
    [
      "ticket",
      "ttl_seconds",
      "http_base_url",
      "server_id",
      "host_public_key_x",
      "host_key_fingerprint",
    ],
    "중앙 등록 티켓"
  );
  if (
    !/^[0-9a-f]{64}$/.test(String(grant.ticket)) ||
    !Number.isSafeInteger(grant.ttl_seconds) ||
    Number(grant.ttl_seconds) < 1 ||
    typeof grant.http_base_url !== "string" ||
    typeof grant.server_id !== "string" ||
    !UUID_PATTERN.test(grant.server_id) ||
    !/^[A-Za-z0-9_-]{43}$/.test(String(grant.host_public_key_x)) ||
    !/^[A-Za-z0-9_-]{43}$/.test(String(grant.host_key_fingerprint))
  ) {
    throw new Error("중앙 등록 티켓 권위가 올바르지 않습니다.");
  }
  return grant as unknown as DesktopCentralRegistrationTicket;
}

function rememberDesktopCentralRegistration(
  ticket: DesktopCentralRegistrationTicket
): DesktopCentralRegistrationTicket {
  rememberDesktopOperatorRuntime(ticket);
  return ticket;
}

export async function requestDesktopRuntimeTicket(
  roomId: string
): Promise<DesktopRuntimeTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  requireDesktopHostCommand("runtime_ticket");
  return tauri
    .invoke<DesktopRuntimeTicket>("runtime_ticket", { roomId })
    .then(rememberDesktopRuntime);
}

export async function requestDesktopBootstrapStatus(): Promise<DesktopBootstrapGrant> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  requireDesktopHostCommand("runtime_bootstrap_status");
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
  requireDesktopHostCommand("runtime_bootstrap_initialize");
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
  requireDesktopHostCommand("runtime_operator_ticket");
  return tauri
    .invoke<DesktopOperatorHttpTicket>("runtime_operator_ticket")
    .then(rememberDesktopOperatorRuntime);
}

export function requestDesktopPreferencesReadTicket(
  roomId: string
): Promise<DesktopOperatorHttpTicket> {
  return requestDesktopHttpTicket(
    "runtime_preferences_read_ticket",
    { roomId },
    "방 preference read 티켓"
  );
}

export function requestDesktopPreferencesWriteTicket(
  roomId: string
): Promise<DesktopOperatorHttpTicket> {
  return requestDesktopHttpTicket(
    "runtime_preferences_write_ticket",
    { roomId },
    "방 preference write 티켓"
  );
}

export function requestDesktopHumanInviteCreateTicket(
  roomId: string
): Promise<DesktopOperatorHttpTicket> {
  return requestDesktopHttpTicket(
    "runtime_human_invite_create_ticket",
    { roomId },
    "사람 초대 생성 티켓"
  );
}

export function requestDesktopHumanInviteRevokeTicket(
  roomId: string
): Promise<DesktopOperatorHttpTicket> {
  return requestDesktopHttpTicket(
    "runtime_human_invite_revoke_ticket",
    { roomId },
    "사람 초대 취소 티켓"
  );
}

export function requestDesktopSettingsDirectoryReadTicket(): Promise<DesktopOperatorHttpTicket> {
  return requestDesktopHttpTicket(
    "runtime_settings_directory_read_ticket",
    undefined,
    "방 settings directory 티켓"
  );
}

export async function fetchDesktopRoomPreferences(
  roomId: string,
  init: RequestInit = {}
): Promise<Response> {
  const method = String(init.method || "GET").toUpperCase();
  if (method !== "GET" && method !== "POST") {
    throw new Error("방 preference 요청은 GET 또는 POST만 허용합니다.");
  }
  if (method === "GET" && init.body !== undefined && init.body !== null) {
    throw new Error("방 preference GET 요청에는 body를 보낼 수 없습니다.");
  }
  const issued =
    method === "GET"
      ? await requestDesktopPreferencesReadTicket(roomId)
      : await requestDesktopPreferencesWriteTicket(roomId);
  const query = method === "GET" ? `?room_id=${encodeURIComponent(roomId)}` : "";
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${issued.ticket}`);
  return fetch(`${issued.http_base_url}/api/room-settings${query}`, {
    ...init,
    cache: "no-store",
    method,
    headers,
  });
}

async function fetchDesktopHumanInvite(
  roomId: string,
  operation: "create" | "revoke",
  init: RequestInit
): Promise<Response> {
  const method = init.method === undefined ? "POST" : String(init.method).toUpperCase();
  if (method !== "POST") {
    throw new Error("사람 초대 요청은 POST만 허용합니다.");
  }
  const issued =
    operation === "create"
      ? await requestDesktopHumanInviteCreateTicket(roomId)
      : await requestDesktopHumanInviteRevokeTicket(roomId);
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${issued.ticket}`);
  return fetch(`${issued.http_base_url}/api/room-invite/${operation}`, {
    ...init,
    cache: "no-store",
    method: "POST",
    headers,
  });
}

export function fetchDesktopHumanInviteCreate(
  roomId: string,
  init: RequestInit
): Promise<Response> {
  return fetchDesktopHumanInvite(roomId, "create", init);
}

export function fetchDesktopHumanInviteRevoke(
  roomId: string,
  init: RequestInit
): Promise<Response> {
  return fetchDesktopHumanInvite(roomId, "revoke", init);
}

export async function requestDesktopCentralRegistrationTicket(): Promise<DesktopCentralRegistrationTicket> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 Rust 런타임을 사용할 수 없습니다.");
  }
  requireDesktopHostCommand("runtime_central_registration_ticket");
  return tauri
    .invoke<unknown>("runtime_central_registration_ticket")
    .then(validateDesktopCentralRegistrationTicket)
    .then(rememberDesktopCentralRegistration);
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

export async function fetchDesktopCentralRegistration(
  init: RequestInit = {}
): Promise<DesktopCentralRegistrationResponse> {
  if (init.method !== "POST") {
    throw new Error("중앙 등록 증명은 POST 요청만 허용합니다.");
  }
  const issued = await requestDesktopCentralRegistrationTicket();
  const headers = new Headers(init.headers);
  headers.set("Authorization", `Bearer ${issued.ticket}`);
  const response = await fetch(
    `${desktopRuntimeHttpBase}/api/central-directory/registration-proof`,
    { ...init, headers }
  );
  return {
    response,
    binding: {
      server_id: issued.server_id,
      host_public_key_x: issued.host_public_key_x,
      host_key_fingerprint: issued.host_key_fingerprint,
    },
  };
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
  requireDesktopHostCommand("choose_local_workspace");
  return tauri.invoke<DesktopWorkspaceSelection>("choose_local_workspace");
}

export async function openDesktopCentralGoogleLogin(url: string): Promise<void> {
  const tauri = tauriInternals();
  if (!tauri) {
    throw new Error("데스크톱 중앙 로그인 기능을 사용할 수 없습니다.");
  }
  requireDesktopHostCommand("open_central_google_login");
  await tauri.invoke("open_central_google_login", { url });
}

export async function cacheNativeRoomDirectory(rooms: unknown[]): Promise<void> {
  const tauri = tauriInternals();
  if (!tauri) return;
  requireDesktopHostCommand("cache_selected_room_directory");
  await tauri.invoke("cache_selected_room_directory", {
    rooms: JSON.stringify(rooms),
  });
}
