import type { RoomAppearance } from "../lib/roomAppearance";
import {
  parseOperatorPairingRedeemResponse,
  parseRoomInviteAdmissionResponse,
  parseRoomInviteJoinResponse,
  type OperatorPairingRedeemResponse,
  type RoomInviteAdmissionResponse,
  type RoomInviteJoinResponse,
} from "../lib/roomAdmissionContract";
import {
  fetchJsonServerOperator,
  postJson,
  postJsonModerator,
  postEmptyServerOperator,
  postJsonWithIdentity,
  postJsonWithToken,
} from "./http";
import {
  parsePublicIngressStatus,
  type PublicIngressStatus,
} from "../lib/publicIngressStatus";

export interface RoomInviteCreateResponse {
  invite_id: string;
  invite_token: string;
  meeting_id: string;
  agent_id: string;
  display_name: string;
  invite_scope: RoomAppearance["inviteScope"];
  expires_at: string;
  room_url: string;
  join_url?: string;
  remote_client_packet?: Record<string, unknown>;
  client_type?: string;
  provider_kind?: string;
}

export type { OperatorPairingRedeemResponse, RoomInviteJoinResponse };

export type { RoomInviteAdmissionResponse };

export type PublicInviteStatus = PublicIngressStatus;

export interface OperatorPairingCreateResponse {
  status: "created";
  pairing_id: string;
  room_id: string;
  target_origin: string;
  expires_at: string;
  pairing_url: string;
}

export function createRoomInvite({
  meetingId,
  agentId,
  displayName,
  inviteScope = "room",
  ttlSeconds = 604800,
  clientType = "browser",
  providerKind = "manual",
  participantType = "human",
  maxUses = 0,
  sessionToken = "",
}: {
  meetingId: string;
  agentId: string;
  displayName: string;
  inviteScope?: RoomAppearance["inviteScope"];
  ttlSeconds?: number;
  clientType?: "browser" | "agent_bridge";
  providerKind?: string;
  participantType?: "human" | "agent";
  maxUses?: number;
  sessionToken?: string;
}) {
  return postJsonModerator<RoomInviteCreateResponse>(
    "/api/room-invite/create",
    {
      meeting_id: meetingId,
      agent_id: agentId,
      display_name: displayName,
      invite_scope: inviteScope,
      ttl_seconds: ttlSeconds,
      client_type: clientType,
      provider_kind: providerKind,
      participant_type: participantType,
      max_uses: maxUses,
    },
    sessionToken
  );
}

export function fetchPublicInviteStatus() {
  return fetchJsonServerOperator<unknown>("/api/public-invite/status").then(
    parsePublicIngressStatus
  );
}

export function startPublicInviteTunnel() {
  return postEmptyServerOperator<unknown>("/api/public-invite/tunnel/start").then(
    parsePublicIngressStatus
  );
}

export function stopPublicInviteTunnel() {
  return postEmptyServerOperator<unknown>("/api/public-invite/tunnel/stop").then(
    parsePublicIngressStatus
  );
}

export function joinRoomInvite({
  inviteToken,
  requestId,
  meetingId,
  displayName,
  avatarImage,
  deviceToken,
  clientId,
  participantType = "human",
}: {
  inviteToken: string;
  requestId: string;
  meetingId: string;
  displayName?: string;
  avatarImage?: string;
  deviceToken?: string;
  clientId: string;
  participantType?: "human" | "agent";
}) {
  return postJson<unknown>("/api/room-invite/join", {
    invite_token: inviteToken,
    request_id: requestId,
    meeting_id: meetingId,
    display_name: displayName,
    avatar_image_url: avatarImage,
    device_token: deviceToken,
    client_id: clientId,
    participant_type: participantType,
  }).then((payload) =>
    parseRoomInviteJoinResponse(payload, requestId, meetingId, clientId)
  );
}

export function preflightRoomInvite({
  inviteToken,
  deviceToken,
  sessionToken = "",
}: {
  inviteToken: string;
  deviceToken: string;
  sessionToken?: string;
}) {
  return postJsonWithIdentity<unknown>(
    "/api/room-invite/admission",
    { invite_token: inviteToken },
    { deviceToken, sessionToken }
  ).then(parseRoomInviteAdmissionResponse);
}

export function createOperatorPairing({
  meetingId,
  sessionToken = "",
}: {
  meetingId: string;
  sessionToken?: string;
}) {
  return postJsonModerator<OperatorPairingCreateResponse>(
    "/api/operator-pairing/create",
    { meeting_id: meetingId, ttl_seconds: 120 },
    sessionToken
  );
}

export function redeemOperatorPairing({
  pairingToken,
  deviceToken,
}: {
  pairingToken: string;
  deviceToken: string;
}) {
  return postJsonWithIdentity<unknown>(
    "/api/operator-pairing/redeem",
    { pairing_token: pairingToken },
    { deviceToken }
  ).then(parseOperatorPairingRedeemResponse);
}

export function createCompanionRoomInvite({
  sessionToken,
  agentId,
  displayName,
  ttlSeconds = 600,
}: {
  sessionToken: string;
  agentId: string;
  displayName: string;
  ttlSeconds?: number;
}) {
  return postJsonWithToken<RoomInviteCreateResponse>(
    "/api/room-invite/companion",
    {
      agent_id: agentId,
      display_name: displayName,
      ttl_seconds: ttlSeconds,
    },
    sessionToken
  );
}

export function leaveRoomInvite({ sessionToken }: { sessionToken: string }) {
  return postJsonWithToken<{ status: string }>("/api/room-invite/leave", {}, sessionToken);
}
