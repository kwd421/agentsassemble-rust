import type {
  LobbyAttachmentRef,
  LobbyEvent,
  LobbyPostResponse,
  RoomAgentSession,
  RoomEvent,
  RoomMember,
  RoomSocketAuth,
  ServerRoom,
} from "./api";
import type { PublicRoomGlobalSettings } from "./types/generatedRoomEvent";
import type { PluginEnvelope } from "./pluginSocketProtocol";
import type { CommandAck } from "./types/generated/CommandAck";
import type { CommandResolution } from "./types/generated/CommandResolution";
import type { ServerProductSurface } from "./types/generated/ServerProductSurface";
import type { RoomRuntimeTicket } from "./lib/roomRuntimeTicket";

export interface RoomSocketHandlers {
  onLobby?: (events: LobbyEvent[]) => void;
  onRoster?: (members: RoomMember[]) => void;
  onRoomSnapshot?: (
    snapshot: RoomSocketSnapshot,
    displayResourceBase: string
  ) => boolean | void;
  onProviderCatalog?: (catalog: ProviderCatalogSnapshot) => void;
  onRoomEvents?: (events: RoomEvent[]) => void;
  onPlugin?: (events: PluginEnvelope[], snapshot: boolean) => void;
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (err: Event | Error) => void;
}

export interface RoomSayRequest {
  message: string;
  attachments?: LobbyAttachmentRef[];
  kind?: "message" | "vote" | "vote_cast" | "vote_withdraw" | "vote_close";
  voteId?: string;
  voteQuestion?: string;
  voteOptions?: string[];
  voteChoice?: string;
  voteDurationSeconds?: number;
}

export class RoomSocketSayError extends Error {
  category: string;

  constructor(message: string, category = "rejected") {
    super(message);
    this.name = "RoomSocketSayError";
    this.category = category;
  }
}

export interface RoomSocketHandle {
  close: () => void;
  resync?: () => void;
  ready: () => boolean;
  say: (request: RoomSayRequest) => Promise<LobbyPostResponse>;
  command: (action: string, payload?: Record<string, unknown>) => Promise<RoomCommandAck>;
  plugin?: (payload: Record<string, unknown>) => void;
  historyBefore: (beforeSeq: number, limit?: number) => Promise<RoomHistoryPage>;
}

export interface RoomHistoryPage {
  events: RoomEvent[];
  oldest_seq: number;
  last_seq: number;
  has_more_before: boolean;
}

export interface NativeCliProviderAvailability {
  id: string;
  display_name: string;
  provider_kind: string;
  runtime_kind: "live_cli" | "opencode" | "api";
  catalog_group: "harness" | "api" | "local";
  workspace_required?: boolean;
  work_harness_available?: boolean;
  custom_endpoint?: boolean;
  custom_model?: boolean;
  connection_kind: "native_cli_bridge";
  executable: string;
  default_model: string;
  interactive: true;
  startable: boolean;
  available: boolean;
  discovery_status?: "loading" | "ready" | "failed";
  catalog_source?: "discovered" | "static_manifest" | "stale_cache";
  discovery_error_code?: string;
  discovery_error?: string;
  login_available?: boolean;
  login_label?: string;
  login_flow?: "browser_oauth" | "interactive_terminal";
  controls: ProviderControl[];
}

export interface ProviderCatalogSnapshot {
  status: "loading" | "ready" | "failed";
  catalog_revision: string;
  discovered_at?: string;
  providers: NativeCliProviderAvailability[];
}

export interface ProviderControlOption {
  value: string;
  label: string;
  metadata?: Record<string, unknown>;
}

export interface ProviderControl {
  key: string;
  label: string;
  kind: "select" | "combobox";
  options: ProviderControlOption[];
  default_value: string;
}

export interface RoomSocketSnapshot {
  op: "snapshot";
  stream: "room_events";
  room: ServerRoom | Record<string, unknown>;
  room_settings: PublicRoomGlobalSettings;
  participants: RoomMember[];
  agent_sessions: RoomAgentSession[];
  active_turns: Array<Record<string, unknown>>;
  events: RoomEvent[];
  oldest_seq: number;
  last_seq: number;
  has_more_before: boolean;
  resume_gap: boolean;
  snapshot_mode: "initial" | "resume" | "gap";
  provider_catalog: ProviderCatalogSnapshot;
  available_providers: NativeCliProviderAvailability[];
  capabilities: Record<string, boolean>;
}

export type RoomCommandAck = Pick<
  CommandAck,
  "request_id" | "action" | "deduplicated"
> & {
  op: "ack";
  accepted: true;
  resolution: Extract<CommandResolution, "committed">;
  result?: Record<string, unknown>;
};

export interface RoomSocketClientDependencies {
  getTicket?: (auth: RoomSocketAuth) => Promise<RoomRuntimeTicket>;
  createSocket?: (url: string) => WebSocket;
  serverSurface: ServerProductSurface;
  expectedRoomId: string;
  expectedParticipantId: string;
}
