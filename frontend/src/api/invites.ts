import { browserDeviceDescription } from "../lib/ownerDeviceDescription";
import { parseMemberChallenge, type MemberHandoff } from "../lib/central/memberConnect";
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
  postEmptyServerOperator,
  postJsonWithIdentity,
} from "./http";
import {
  parsePublicIngressStatus,
  type PublicIngressStatus,
} from "../lib/publicIngressStatus";

export type { OperatorPairingRedeemResponse, RoomInviteJoinResponse };

export type { RoomInviteAdmissionResponse };

export type PublicInviteStatus = PublicIngressStatus;

export function fetchPublicInviteStatus(beforeDispatch?: () => void) {
  return fetchJsonServerOperator<unknown>("/api/public-invite/status", beforeDispatch).then(
    parsePublicIngressStatus
  );
}

export function startPublicInviteTunnel(beforeDispatch?: () => void) {
  return postEmptyServerOperator<unknown>(
    "/api/public-invite/tunnel/start",
    beforeDispatch
  ).then(parsePublicIngressStatus);
}

export function stopPublicInviteTunnel(beforeDispatch?: () => void) {
  return postEmptyServerOperator<unknown>(
    "/api/public-invite/tunnel/stop",
    beforeDispatch
  ).then(parsePublicIngressStatus);
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
  participantType?: "human";
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

export async function challengeRoomMember(inviteToken: string, deviceToken: string) {
  return parseMemberChallenge(await postJsonWithIdentity<unknown>(
    "/api/room-invite/member-challenge", { invite_token: inviteToken }, { deviceToken }
  ));
}

export async function joinRoomMember(record: MemberHandoff, grantToken: string,
  requestId: string, clientId: string, deviceToken: string) {
  const payload = await postJsonWithIdentity<unknown>("/api/room-invite/member-join", {
    invite_token: record.invite_token, challenge_id: record.challenge_id,
    grant_token: grantToken, request_id: requestId, client_id: clientId,
  }, { deviceToken });
  return parseRoomInviteJoinResponse(payload, requestId, record.meeting_id, clientId);
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
    { pairing_token: pairingToken, device: browserDeviceDescription() },
    { deviceToken }
  ).then(parseOperatorPairingRedeemResponse);
}

export function leaveRoomInvite(identity: { sessionToken: string; deviceToken?: string }) {
  return postJsonWithIdentity<{ status: string }>("/api/room-invite/leave", {}, identity);
}
