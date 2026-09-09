// Aggregate exports for the current frontend API client.
import { fetchJson, postJson } from "./api/http";
export { chooseDesktopWorkspace as chooseLocalWorkspace } from "./lib/desktopBridge";

export * from "./api/agentSessions";
export * from "./api/humanInviteManager";
export * from "./api/invites";
export * from "./api/identity";
export * from "./api/messagePins";
export * from "./api/messageAttachments";
export * from "./api/messageSearch";
export * from "./api/personas";
export * from "./api/providerCredentials";
export * from "./api/room";
export * from "./api/roomAppearance";
export * from "./api/roomHistory";
export * from "./api/roomHttpAuthority";
export * from "./api/userProfile";
export { clearHostToken, loadHostToken, postJsonHost, saveHostToken } from "./api/http";

export interface LiveAgent {
  agent_id: string;
  display_name: string;
  avatar_image_url?: string;
  owner_id?: string;
  created_by?: string;
  owner_display_name?: string;
  owner_participant_id?: string;
  owner_session_id?: string;
  status: string;
  provider_kind: string;
  connection_kind?: string;
  engagement_mode?: string;
  meeting_id: string;
  session_id?: string;
  model_id?: string;
  effort?: string;
  speed?: string;
  process_group_id?: string;
  live_agent_config_path?: string;
  workspace_path?: string;
  last_seen_at?: string;
  last_reply_at?: string;
  last_observed_event_id?: string;
  last_observed_live_event_id?: string;
  poll_interval?: number;
  poll_interval_updated_at?: string;
  cooldown?: number;
  cooldown_updated_at?: string;
  permission_option?: string;
  fast_mode?: boolean;
  relaunch_pid?: number;
  relaunch_host?: string;
  relaunch_argv?: string[];
  relaunch_cwd?: string;
  persona_card_id?: string;
  character_mode?: string;
  join_semantics?: string;
  context_durability?: string;
  execution_mode?: "baseline_call_resume" | "runtime_managed_room_turn" | "provider_tool_loop" | "tool_loop_unverified" | "call" | "call_resume" | "persistent" | "provider_persistent" | "manual" | "unknown" | string;
  runner_residency?: string;
  provider_residency?: string;
  provider_persistent?: boolean;
  execution_summary?: string;
  tool_loop_unverified_reason?: string;
  sandbox_enforcement?: string;
  admission_status?: string;
  host_approved_binding?: boolean;
  binding_role_id?: string;
  binding_permission_profile_id?: string;
  binding_join_mode?: string;
  binding_conflicts?: string[];
  capabilities?: string[];
}

export interface MafiaPlayer {
  agent_id: string;
  display_name: string;
  alive: boolean;
  role?: string;
  team?: string;
}

export interface MafiaEvent {
  id: string;
  created_at: string;
  kind: string;
  channel: "all" | "mafia_team";
  actor_id: string;
  name: string;
  message: string;
  phase: string;
  day_number: number;
}

export interface MafiaGame {
  game_id: string;
  status: string;
  phase: string;
  day_number: number;
  winner: string;
  players: MafiaPlayer[];
  events: MafiaEvent[];
  viewer?: {
    agent_id: string;
    role: string;
    team: string;
  };
}

export interface MafiaGameResponse {
  game: MafiaGame | null;
}

export function fetchMafiaGame(gameId: string, viewerAgentId = "") {
  const query = new URLSearchParams({
    game_id: gameId,
    viewer_agent_id: viewerAgentId,
  });
  return fetchJson<MafiaGameResponse>(`/api/play/mafia?${query.toString()}`);
}

export function startMafiaGame(params: {
  game_id: string;
  players: Array<{ agent_id: string; display_name: string }>;
  mafia_count?: number;
}) {
  return postJson<MafiaGameResponse>("/api/play/mafia/start", params);
}

export function sendMafiaChat(params: {
  game_id: string;
  speaker_id: string;
  channel: "all" | "mafia_team";
  message: string;
  viewer_agent_id?: string;
}) {
  return postJson<MafiaGameResponse & { event?: MafiaEvent }>("/api/play/mafia/chat", params);
}

export function castMafiaVote(params: {
  game_id: string;
  voter_id: string;
  target_id: string;
  viewer_agent_id?: string;
}) {
  return postJson<MafiaGameResponse & { event?: MafiaEvent }>("/api/play/mafia/vote", params);
}

export function resolveMafiaPhase(gameId: string, viewerAgentId = "") {
  return postJson<MafiaGameResponse>("/api/play/mafia/resolve", {
    game_id: gameId,
    viewer_agent_id: viewerAgentId,
  });
}
