import type {
  LobbyAttachmentRef,
  LobbyEvent,
  LobbyPostResponse,
  RoomEvent,
  RoomMember,
  RoomSocketAuth,
  SideChatEvent,
} from "./api";
import type { PluginEnvelope } from "./pluginSocketProtocol";
import type { ProviderAvailability } from "./types/generated/ProviderAvailability";
import type { ProviderCatalog } from "./types/generated/ProviderCatalog";
import type { ProviderControl as GeneratedProviderControl } from "./types/generated/ProviderControl";
import type { ProviderControlOption as GeneratedProviderControlOption } from "./types/generated/ProviderControlOption";
import type { RoomSnapshot } from "./types/generated/RoomSnapshot";
import type { TicketResponse } from "./types/generated/TicketResponse";

export interface RoomSocketHandlers {
  onLobby?: (events: LobbyEvent[]) => void;
  onRoster?: (members: RoomMember[]) => void;
  onSideChat?: (events: SideChatEvent[]) => void;
  onRoomSnapshot?: (snapshot: RoomSocketSnapshot) => boolean | void;
  onProviderCatalog?: (catalog: ProviderCatalogSnapshot) => void;
  onRoomEvents?: (events: RoomEvent[]) => void;
  onPlugin?: (events: PluginEnvelope[], snapshot: boolean) => void;
  onRoomDeleted?: (roomId: string, roomName: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (err: Event | Error) => void;
}

export interface RoomSayRequest {
  message: string;
  attachments?: LobbyAttachmentRef[];
  kind?: "message" | "ready" | "deploy" | "vote" | "vote_cast" | "vote_withdraw" | "vote_close";
  voteId?: string;
  voteQuestion?: string;
  voteOptions?: string[];
  voteChoice?: string;
  voteDurationSeconds?: number;
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

export type NativeCliProviderAvailability = ProviderAvailability;
export type ProviderCatalogSnapshot = ProviderCatalog;
export type ProviderControlOption = GeneratedProviderControlOption;
export type ProviderControl = GeneratedProviderControl;
export type RoomSocketSnapshot = RoomSnapshot & {
  op: "snapshot";
};

export interface RoomCommandAck {
  op: "ack";
  request_id: string;
  accepted: true;
  action: string;
  result?: Record<string, unknown>;
  deduplicated?: boolean;
}

export interface RoomSocketClientDependencies {
  getTicket?: (auth: RoomSocketAuth) => Promise<TicketResponse | string>;
  createSocket?: (url: string) => WebSocket;
  websocketBaseUrl?: () => string;
  serverProofKey?: () => string;
}
