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

export async function challengeMemberConnect(deviceToken: string) {
  return parseMemberChallenge(await postJsonWithIdentity<unknown>("/api/member-connect/challenge", {}, {deviceToken}));
}
export async function redeemMemberConnect(record: MemberHandoff, grantToken: string, deviceToken: string) {
  const value = await postJsonWithIdentity<{rooms: {room_id:string;name:string}[]}>("/api/member-connect/rooms", {challenge_id:record.challenge_id,grant_token:grantToken}, {deviceToken});
  if (!Array.isArray(value.rooms) || value.rooms.length > 50 || !value.rooms.every(r => typeof r.room_id === "string" && r.room_id && typeof r.name === "string")) throw new Error("참가 중인 방을 확인하지 못했어요.");
  return value.rooms;
}
export async function selectMemberConnect(record: MemberHandoff, roomId: string, clientId: string, deviceToken: string) {
  const payload = await postJsonWithIdentity<RoomInviteJoinResponse>("/api/member-connect/select", {challenge_id:record.challenge_id,room_id:roomId,client_id:clientId}, {deviceToken});
  return parseRoomInviteJoinResponse(payload,payload.request_id,roomId,clientId);
}
