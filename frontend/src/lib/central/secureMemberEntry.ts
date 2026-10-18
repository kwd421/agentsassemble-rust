import { HUMAN_INVITE_JOIN_CODE_BYTES, HUMAN_INVITE_JOIN_CODE_PREFIX } from "../../types/generated/HUMAN_INVITE_WIRE";
import { decodeCanonicalBase64Url } from "../base64Url";
import { getOrCreateBrowserCredential, getOrCreateClientId } from "../deviceIdentity";
import { createSecureRequestId } from "../secureRequestId";
import { strictRecord, assertExactKeys } from "../strictJsonContract";
import { parseRoomInviteAdmissionResponse, parseRoomInviteJoinResponse, type RoomInviteJoinResponse } from "../roomAdmissionContract";
import { roomGuestSessionFromJoinPayload } from "../roomGuestSession";
import { RemoteTransport } from "../remote/remoteTransport";
import { installRemoteWorkspace, bindRemoteSession } from "../remote/remoteWorkspace";
import { SECURE_PROTOCOL, type SecureTarget } from "../remote/secureCrypto";
import { loadCentralSession, signedRequest, type CentralSession } from "./identity";
import { parseMemberChallenge } from "./memberConnect";
import { responseError } from "../../api/http";

import type { SecureMemberEntry } from "../remote/remoteWorkspace";
export type { SecureMemberEntry } from "../remote/remoteWorkspace";
export function consumeSecureMemberEntry(): SecureMemberEntry | undefined {
  const url = new URL(window.location.href);
  if (url.pathname !== "/member-join" || !url.searchParams.has("protocol")) return;
  window.history.replaceState({}, "", "/member-join");
  if (url.hash || [...url.searchParams.keys()].sort().join() !== "protocol,registration_epoch,server_id,token" ||
      url.searchParams.get("protocol") !== SECURE_PROTOCOL) throw new Error("초대 링크를 확인하지 못했어요. 원래 링크를 다시 열어 주세요.");
  const server_id = url.searchParams.get("server_id") || "", registration_epoch = url.searchParams.get("registration_epoch") || "", inviteToken = url.searchParams.get("token") || "";
  if (!/^[A-Za-z0-9._:-]{1,200}$/.test(server_id) || !/^[A-Za-z0-9._:-]{1,200}$/.test(registration_epoch) || (!inviteToken.startsWith(HUMAN_INVITE_JOIN_CODE_PREFIX) || decodeCanonicalBase64Url(inviteToken.slice(HUMAN_INVITE_JOIN_CODE_PREFIX.length))?.length !== HUMAN_INVITE_JOIN_CODE_BYTES)) throw new Error("초대 링크를 확인하지 못했어요. 원래 링크를 다시 열어 주세요.");
  return { server_id, registration_epoch, inviteToken };
}

export async function previewSecureMember(entry: SecureMemberEntry, signal?: AbortSignal) {
  const session = loadCentralSession(); if (!session) throw new Error("로그인이 필요해요.");
  const value = strictRecord(await signedRequest(session, `/v1/servers/${encodeURIComponent(entry.server_id)}/member-preview`, "POST", { protocol: SECURE_PROTOCOL, registration_epoch: entry.registration_epoch, ...(entry.inviteToken ? {resolve_invite:true} : {}) }, signal), "입장할 서버");
  assertExactKeys(value, ["server_id", "label", "endpoint_origin", "endpoint_generation", "protocol", "mode", "registration_epoch", "host_key_fingerprint", "host_public_key_jwk"], "입장할 서버");
  if (value.server_id !== entry.server_id || (!entry.inviteToken && value.registration_epoch !== entry.registration_epoch) || typeof value.registration_epoch!=="string" || !/^[A-Za-z0-9._:-]{1,200}$/.test(value.registration_epoch) || value.protocol !== SECURE_PROTOCOL || value.mode !== "event_secure_v1" ||
      typeof value.label !== "string" || typeof value.endpoint_origin !== "string" || typeof value.host_key_fingerprint !== "string" || !Number.isSafeInteger(value.endpoint_generation) ||
      loadCentralSession()?.token !== session.token) throw new Error("입장할 서버를 확인하지 못했어요.");
  const target: SecureTarget = { server_id: entry.server_id, registration_epoch: value.registration_epoch, origin: value.endpoint_origin,
    generation: Number(value.endpoint_generation), host_key_fingerprint: value.host_key_fingerprint, host_public_key_jwk: value.host_public_key_jwk as JsonWebKey };
  return { target, label: value.label, endpoint_origin: target.origin, endpoint_generation: target.generation, server_id: entry.server_id };
}

export class SecureMemberAdmission {
  readonly transport: RemoteTransport;
  readonly deviceToken: string;
  readonly clientId = getOrCreateClientId();
  private challengeId = "";
  private challengeExpiresAt = 0;
  private roomSelectReuse = false;
  private session: CentralSession;
  private entry: SecureMemberEntry;
  private adopted = false;
  private constructor(transport: RemoteTransport, session: CentralSession, entry: SecureMemberEntry) {
    this.transport = transport; this.session = session; this.entry = entry;
    this.deviceToken = getOrCreateBrowserCredential({ server_id: entry.server_id, registration_epoch: entry.registration_epoch });
  }
  static async open(entry: SecureMemberEntry, target: SecureTarget, sessionToken: string, signal: AbortSignal) {
    const session = loadCentralSession(); if (!session || session.token !== sessionToken) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
    if(target.server_id!==entry.server_id || (!entry.inviteToken && target.registration_epoch!==entry.registration_epoch))throw new Error("입장할 서버를 확인하지 못했어요.");
    return new SecureMemberAdmission(await RemoteTransport.connect(target, entry.inviteToken ? "member_admission" : "member_connect", signal), session, {...entry,registration_epoch:target.registration_epoch});
  }
  close() { if (!this.adopted) this.transport.close(); }
  private async post(path: string, body: object, signal?: AbortSignal) {
    const response = await this.transport.fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "X-Device-Token": this.deviceToken }, body: JSON.stringify(body), ...(signal ? { signal } : {}) });
    if (!response.ok) throw await responseError(response);
    return response.json();
  }
  async admit(signal?: AbortSignal): Promise<{ room_id: string; name: string }[] | null> {
    const invite = this.entry.inviteToken;
    if (invite) {
      const challenge = parseMemberChallenge(await this.post("/api/room-invite/member-challenge", { invite_token: invite }, signal));
      if (challenge.server_id !== this.entry.server_id || challenge.registration_epoch !== this.entry.registration_epoch) throw new Error("서버 주소가 변경됐어요. 다시 연결해 주세요.");
      this.challengeId = challenge.challenge_id;
      const { protocol, client_public_key, channel_id, origin, generation } = this.transport.hello;
      const binding = { protocol, client_public_key, channel_id, origin, generation, purpose: "admission", registration_epoch: this.entry.registration_epoch };
      const grant = strictRecord(await signedRequest(this.session, `/v1/servers/${encodeURIComponent(this.entry.server_id)}/member-grants`, "POST", { ...binding, challenge_hash: challenge.challenge_hash }, signal), "서버 입장권");
      assertExactKeys(grant, ["grant_token", "server_id", "registration_epoch", "expires_at", "endpoint_origin", "endpoint_generation", "protocol", "client_public_key", "channel_id", "purpose"], "서버 입장권");
      if (["protocol", "client_public_key", "channel_id", "purpose", "registration_epoch"].some(key => grant[key] !== binding[key as keyof typeof binding]) ||
          grant.server_id !== this.entry.server_id || grant.endpoint_origin !== origin || grant.endpoint_generation !== generation ||
          typeof grant.grant_token !== "string" || !/^aamg1\.[A-Za-z0-9_-]{43}$/.test(grant.grant_token) ||
          !Number.isSafeInteger(grant.expires_at) || Number(grant.expires_at) <= Date.now() / 1000 || loadCentralSession()?.token !== this.session.token) throw new Error("입장권을 확인하지 못했어요. 다시 시도해 주세요.");
      const preflight = parseRoomInviteAdmissionResponse(await this.post("/api/room-invite/admission", { invite_token: invite }));
      if (!("room_id" in preflight)) throw new Error("초대를 사용할 수 없어요. 새 초대를 받아 주세요.");
      const requestId = createSecureRequestId();
      const payload = await this.post("/api/room-invite/member-join", { invite_token: invite, challenge_id: this.challengeId, grant_token: grant.grant_token, request_id: requestId, client_id: this.clientId });
      this.install(payload, requestId, preflight.room_id); return null;
    }
    const { challenge, rooms } = await this.freshConnectChallenge(signal);
    this.challengeId = challenge.challenge_id;
    this.challengeExpiresAt = challenge.expires_at;
    if (!rooms.length) throw new Error("참가 중인 방을 확인하지 못했어요.");
    return rooms;
  }
  async select(roomId: string, signal?: AbortSignal): Promise<RoomInviteJoinResponse> {
    const payload = await this.post("/api/member-connect/select", { challenge_id: this.challengeId, room_id: roomId, client_id: this.clientId }, signal);
    return this.install(payload, payload.request_id, roomId);
  }
  private async connectGrant(challenge: { challenge_id: string; challenge_hash: string }, signal?: AbortSignal): Promise<string> {
    const { protocol, client_public_key, channel_id, origin, generation } = this.transport.hello;
    const binding = { protocol, client_public_key, channel_id, origin, generation, purpose: "connect", registration_epoch: this.entry.registration_epoch };
    const grant = strictRecord(await signedRequest(this.session, `/v1/servers/${encodeURIComponent(this.entry.server_id)}/member-connect-grants`, "POST", { ...binding, challenge_hash: challenge.challenge_hash }, signal), "서버 입장권");
    assertExactKeys(grant, ["grant_token", "server_id", "registration_epoch", "expires_at", "endpoint_origin", "endpoint_generation", "protocol", "client_public_key", "channel_id", "purpose"], "서버 입장권");
    if (["protocol", "client_public_key", "channel_id", "purpose", "registration_epoch"].some(key => grant[key] !== binding[key as keyof typeof binding]) ||
        grant.server_id !== this.entry.server_id || grant.endpoint_origin !== origin || grant.endpoint_generation !== generation ||
        typeof grant.grant_token !== "string" || !/^aamc1\.[A-Za-z0-9_-]{43}$/.test(grant.grant_token) ||
        !Number.isSafeInteger(grant.expires_at) || Number(grant.expires_at) <= Date.now() / 1000 || loadCentralSession()?.token !== this.session.token) throw new Error("입장권을 확인하지 못했어요. 다시 시도해 주세요.");
    return grant.grant_token;
  }
  /**
   * Background select of one more joined room over the SAME host channel.
   * A host that advertises `room_select_reuse` keeps one redeemed connect
   * challenge usable for every joined room until its window closes, so a
   * reused challenge spends no new central grant and each room is rechecked
   * (join, room, admission currency) at the host on the select itself. An old
   * host (no capability) or a lapsed window takes a fresh challenge+grant and
   * selects on it. The room session is bound for socket tickets without
   * installing a workspace (the workspace keeps the foreground session only).
   * Selecting also adopts the channel, so an attached host is never reaped as
   * admit-only.
   */
  async selectBackgroundRoom(roomId: string, signal?: AbortSignal): Promise<RoomInviteJoinResponse> {
    if (this.roomSelectReuse && this.challengeLive()) {
      return await this.selectWithChallenge(this.challengeId, roomId, signal);
    }
    const { challenge, rooms } = await this.freshConnectChallenge(signal);
    if (!rooms.some((room) => room && typeof room.room_id === "string" && room.room_id === roomId)) throw new Error("참가 중인 방을 확인하지 못했어요.");
    if (this.roomSelectReuse) {
      // This host supports reuse but our window lapsed; adopt the fresh
      // admission so later selects reuse it instead of spending per-room.
      this.challengeId = challenge.challenge_id;
      this.challengeExpiresAt = challenge.expires_at;
    }
    return await this.selectWithChallenge(challenge.challenge_id, roomId, signal);
  }
  private async freshConnectChallenge(signal?: AbortSignal): Promise<{ challenge: ReturnType<typeof parseMemberChallenge>; rooms: { room_id: string; name: string }[] }> {
    const challenge = parseMemberChallenge(await this.post("/api/member-connect/challenge", {}, signal));
    if (challenge.server_id !== this.entry.server_id || challenge.registration_epoch !== this.entry.registration_epoch) throw new Error("서버 주소가 변경됐어요. 다시 연결해 주세요.");
    const grantToken = await this.connectGrant(challenge, signal);
    const value = strictRecord(await this.post("/api/member-connect/rooms", { challenge_id: challenge.challenge_id, grant_token: grantToken }, signal), "참가 중인 방");
    assertExactKeys(value, ["rooms"], "참가 중인 방", ["room_select_reuse"]);
    if (!Array.isArray(value.rooms) || value.rooms.length > 50 || !value.rooms.every((room) => room && typeof room.room_id === "string" && room.room_id && typeof room.name === "string")) throw new Error("참가 중인 방을 확인하지 못했어요.");
    // A host that omits the capability keeps the per-room grant path, so the
    // reuse flag is only ever raised by a host that vouches for it.
    this.roomSelectReuse = value.room_select_reuse === true;
    return { challenge, rooms: value.rooms as { room_id: string; name: string }[] };
  }
  private challengeLive(): boolean {
    // A select must reach the host before its challenge window closes; keep a
    // small margin so the reuse decision never races the host-side expiry.
    return this.challengeExpiresAt > Date.now() / 1000 + 5;
  }
  private async selectWithChallenge(challengeId: string, roomId: string, signal?: AbortSignal): Promise<RoomInviteJoinResponse> {
    const payload = await this.post("/api/member-connect/select", { challenge_id: challengeId, room_id: roomId, client_id: this.clientId }, signal);
    const parsed = parseRoomInviteJoinResponse(payload, payload.request_id, roomId, this.clientId);
    if (parsed.server_id !== this.entry.server_id || loadCentralSession()?.token !== this.session.token || !this.transport.active) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
    bindRemoteSession(parsed.session_token, this.transport, parsed.server_id);
    return parsed;
  }
  private install(raw: unknown, requestId: string, roomId: string): RoomInviteJoinResponse {
    const payload = parseRoomInviteJoinResponse(raw, requestId, roomId, this.clientId);
    if (payload.server_id !== this.entry.server_id || loadCentralSession()?.token !== this.session.token || !this.transport.active) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
    const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true };
    bindRemoteSession(member.sessionToken, this.transport, member.serverSurface.server_id);
    this.adopted = true;
    installRemoteWorkspace({ transport: this.transport, owner: null, member, deviceToken: this.deviceToken, clientId: this.clientId });
    return payload;
  }
}
