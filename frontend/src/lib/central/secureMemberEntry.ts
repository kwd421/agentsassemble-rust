import { HUMAN_INVITE_JOIN_CODE_BYTES, HUMAN_INVITE_JOIN_CODE_PREFIX } from "../../types/generated/HUMAN_INVITE_WIRE";
import { decodeCanonicalBase64Url } from "../base64Url";
import { getOrCreateBrowserCredential, getOrCreateClientId } from "../deviceIdentity";
import { createSecureRequestId } from "../secureRequestId";
import { strictRecord, assertExactKeys } from "../strictJsonContract";
import { parseRoomInviteAdmissionResponse, parseRoomInviteJoinResponse, type RoomInviteJoinResponse } from "../roomAdmissionContract";
import { roomGuestSessionFromJoinPayload } from "../roomGuestSession";
import { RemoteTransport } from "../remote/remoteTransport";
import { installRemoteWorkspace, bindRemoteSession, remoteWorkspaceFor } from "../remote/remoteWorkspace";
import { SECURE_PROTOCOL, type SecureTarget } from "../remote/secureCrypto";
import { loadCentralSession, signedRequest, type CentralSession } from "./identity";
import { parseMemberChallenge } from "./memberConnect";
import { noteMemberJoinConsent } from "./memberJoinHandoff";
import {
  MEMBER_RESUME_PURPOSE,
  browserCredentialFingerprint,
  memberResumeTranscript,
  parseMemberResumeAnswer,
  parseMemberResumeNonce,
  signMemberResume,
  type MemberResumeTuple,
} from "./memberResume";
import { loadMemberResumeKey, loadOrCreateMemberResumeKey } from "./memberResumeKey";
import { MemberAnchorMismatchError } from "./memberAnchors";
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
  // The host serves another incarnation: the stored anchor no longer
  // matches, so the host is parked for a fresh consent instead of being
  // retried against an incarnation the person never joined.
  if (typeof value.registration_epoch === "string" && /^[A-Za-z0-9._:-]{1,200}$/.test(value.registration_epoch) &&
      !entry.inviteToken && value.registration_epoch !== entry.registration_epoch) throw new MemberAnchorMismatchError();
  if (value.server_id !== entry.server_id || typeof value.registration_epoch!=="string" || !/^[A-Za-z0-9._:-]{1,200}$/.test(value.registration_epoch) || value.protocol !== SECURE_PROTOCOL || value.mode !== "event_secure_v1" ||
      typeof value.label !== "string" || typeof value.endpoint_origin !== "string" || typeof value.host_key_fingerprint !== "string" || !Number.isSafeInteger(value.endpoint_generation) ||
      loadCentralSession()?.token !== session.token) throw new Error("입장할 서버를 확인하지 못했어요.");
  const target: SecureTarget = { server_id: entry.server_id, registration_epoch: value.registration_epoch, origin: value.endpoint_origin,
    generation: Number(value.endpoint_generation), host_key_fingerprint: value.host_key_fingerprint, host_public_key_jwk: value.host_public_key_jwk as JsonWebKey };
  return { target, label: value.label, endpoint_origin: target.origin, endpoint_generation: target.generation, server_id: entry.server_id };
}

/** The name and picture a member chose on the join screen. */
export type MemberJoinProfile = { display_name?: string; avatar_png_base64?: string };

export class SecureMemberAdmission {
  readonly transport: RemoteTransport;
  readonly deviceToken: string;
  readonly clientId = getOrCreateClientId();
  private challengeId = "";
  private challengeExpiresAt = 0;
  private roomSelectReuse = false;
  private roomSelectChannelBound = false;
  /** A resumed admission selects on its own channel: the challenge stays usable. */
  private channelBoundSelect = false;
  /** A select on this live channel stays free past the 300s window (no central grant). */
  get channelBoundSelectFree(): boolean {
    return this.roomSelectChannelBound && this.transport.active;
  }
  private session: CentralSession;
  private entry: SecureMemberEntry;
  /** The incarnation central pinned for this admission (epoch, key, origin). */
  private target: SecureTarget;
  private adopted = false;
  private constructor(transport: RemoteTransport, session: CentralSession, entry: SecureMemberEntry, target: SecureTarget) {
    this.transport = transport; this.session = session; this.entry = entry; this.target = target;
    this.deviceToken = getOrCreateBrowserCredential({ server_id: entry.server_id, registration_epoch: entry.registration_epoch });
  }
  static async open(entry: SecureMemberEntry, target: SecureTarget, sessionToken: string, signal: AbortSignal) {
    const session = loadCentralSession(); if (!session || session.token !== sessionToken) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
    if(target.server_id!==entry.server_id || (!entry.inviteToken && target.registration_epoch!==entry.registration_epoch))throw new Error("입장할 서버를 확인하지 못했어요.");
    return new SecureMemberAdmission(await RemoteTransport.connect(target, entry.inviteToken ? "member_admission" : "member_connect", signal), session, {...entry,registration_epoch:target.registration_epoch}, target);
  }
  close() { if (!this.adopted) this.transport.close(); }
  private async post(path: string, body: object, signal?: AbortSignal) {
    const response = await this.transport.fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "X-Device-Token": this.deviceToken }, body: JSON.stringify(body), ...(signal ? { signal } : {}) });
    if (!response.ok) throw await responseError(response);
    return response.json();
  }
  async admit(signal?: AbortSignal, profile?: MemberJoinProfile): Promise<{ room_id: string; name: string }[] | null> {
    const invite = this.entry.inviteToken;
    if (invite) {
      // Asking with the query lets a newer host say it takes a join profile; an older host ignores it.
      const challenge = parseMemberChallenge(await this.post("/api/room-invite/member-challenge?member_join_profile=1", { invite_token: invite }, signal));
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
      const joinProfile = challenge.member_join_profile && profile ? { profile } : {};
      const payload = await this.post("/api/room-invite/member-join", { invite_token: invite, challenge_id: this.challengeId, grant_token: grant.grant_token, request_id: requestId, client_id: this.clientId, ...joinProfile });
      this.install(payload, requestId, preflight.room_id);
      // This join is the person's explicit consent for this host
      // incarnation: the anchor follows the epoch and host key central pinned
      // for it, and the member host registry may take over this live
      // admission (and lift the epoch/host-key refusal it recorded) through
      // the join handoff. Nothing else creates a handoff.
      noteMemberJoinConsent(this.session.person.person_id, {
        server_id: this.entry.server_id,
        registration_epoch: this.entry.registration_epoch,
        host_key_fingerprint: this.target.host_key_fingerprint,
        endpoint_origin: this.target.origin,
        endpoint_generation: this.target.generation,
      }, this, [{ room_id: payload.meeting_id, name: payload.room_label }]);
      return null;
    }
    const { challenge, rooms } = await this.freshConnectChallenge(signal);
    this.challengeId = challenge.challenge_id;
    this.challengeExpiresAt = challenge.expires_at;
    // The central admission proved this device: the host may now hold its
    // resume key, so the next reconnect needs no central grant. A host
    // without that route simply keeps the central path.
    await this.issueResumeKey(signal);
    if (!rooms.length) throw new Error("참가 중인 방을 확인하지 못했어요.");
    return rooms;
  }
  /**
   * Stores this device's resume key on the host. The host owns the expiry
   * and the identity: the client only contributes its public key.
   */
  private async issueResumeKey(signal?: AbortSignal): Promise<void> {
    const personId = this.session?.person?.person_id || "";
    if (!personId) return;
    let key;
    try {
      key = await loadOrCreateMemberResumeKey(
        personId,
        this.entry.server_id,
        this.entry.registration_epoch,
      );
    } catch {
      return;
    }
    try {
      await this.post("/api/member-connect/resume-key", { client_public_key: key.publicKey }, signal);
    } catch {
      // An old host has no resume route: the central path stays the fallback.
    }
  }
  /**
   * Re-proves this device with its resume key and lists its joined rooms.
   * The signature is built and sent only after the handshake verified the
   * pinned host key, and no central request is spent.
   */
  async resumeDirect(signal?: AbortSignal): Promise<{ room_id: string; name: string }[]> {
    const personId = this.session?.person?.person_id || "";
    if (!personId) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
    const key = await loadMemberResumeKey(
      personId,
      this.entry.server_id,
      this.entry.registration_epoch,
    );
    if (!key) throw new Error("이 기기는 아직 재연결 자격 증명이 없어요.");
    const hello = this.transport.hello;
    const nonce = parseMemberResumeNonce(
      await this.post("/api/member-connect/resume-nonce", {}, signal),
    );
    const tuple: MemberResumeTuple = {
      server_id: hello.server_id,
      registration_epoch: hello.registration_epoch,
      origin: hello.origin,
      generation: hello.generation,
      host_key_fingerprint: this.target.host_key_fingerprint,
      browser_credential_fingerprint: await browserCredentialFingerprint(this.deviceToken),
      client_public_key: hello.client_public_key,
      channel_id: hello.channel_id,
      nonce: nonce.nonce,
    };
    const signature = await signMemberResume(key.privateKey, memberResumeTranscript(tuple));
    const answer = parseMemberResumeAnswer(
      await this.post(
        "/api/member-connect/resume",
        {
          ...tuple,
          // The device's own resume key selects the row the host verifies.
          resume_public_key: key.publicKey,
          purpose: MEMBER_RESUME_PURPOSE,
          signature,
        },
        signal,
      ),
    );
    this.challengeId = answer.challenge_id;
    this.challengeExpiresAt = answer.challenge_expires_at;
    this.channelBoundSelect = true;
    return answer.rooms;
  }
  async select(roomId: string, signal?: AbortSignal): Promise<RoomInviteJoinResponse> {
    // A select that reused the live admission/challenge stays on the same
    // live channel the workspace already holds: never re-install the
    // transport, only bind the new room session. A genuinely new transport
    // (re-attach after a stale challenge) still falls through to install so
    // it replaces and closes the old channel. Past the 300-second window the
    // same live channel stays usable when the host advertised channel-bound
    // selects: the host re-checks join/room/admission/epoch exactly as at
    // mint instead of requiring a fresh central grant.
    const workspaceMatches =
      remoteWorkspaceFor(this.entry.server_id)?.transport === this.transport;
    if (
      workspaceMatches &&
      ((this.roomSelectReuse && this.challengeLive()) ||
        (this.roomSelectChannelBound && this.transport.active))
    ) {
      return await this.selectWithChallenge(this.challengeId, roomId, signal);
    }
    const payload = await this.post(this.channelBoundSelect ? "/api/member-connect/select?room_select_channel_bound=1" : "/api/member-connect/select", { challenge_id: this.challengeId, room_id: roomId, client_id: this.clientId }, signal);
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
    if ((this.roomSelectReuse && this.challengeLive()) ||
        (this.roomSelectChannelBound && this.transport.active)) {
      return await this.selectWithChallenge(this.challengeId, roomId, signal);
    }
    const { challenge, rooms } = await this.freshConnectChallenge(signal);
    if (!rooms.some((room) => room && typeof room.room_id === "string" && room.room_id === roomId)) throw new Error("참가 중인 방을 확인하지 못했어요.");
    if (this.roomSelectReuse || this.roomSelectChannelBound) {
      // This host supports reuse but our window lapsed; adopt the fresh
      // admission so later selects reuse it instead of spending per-room.
      // (Channel-bound selects normally never reach this fallback on a live
      // channel; a dead transport still takes a fresh challenge+grant.)
      this.challengeId = challenge.challenge_id;
      this.challengeExpiresAt = challenge.expires_at;
    }
    return await this.selectWithChallenge(challenge.challenge_id, roomId, signal);
  }
  private async freshConnectChallenge(signal?: AbortSignal): Promise<{ challenge: ReturnType<typeof parseMemberChallenge>; rooms: { room_id: string; name: string }[] }> {
    const challenge = parseMemberChallenge(await this.post("/api/member-connect/challenge", {}, signal));
    if (challenge.server_id !== this.entry.server_id || challenge.registration_epoch !== this.entry.registration_epoch) throw new Error("서버 주소가 변경됐어요. 다시 연결해 주세요.");
    const grantToken = await this.connectGrant(challenge, signal);
    // The reuse advertisement rides the URL query so old hosts (whose
    // `RedeemRequest` body keeps `deny_unknown_fields`) accept the request
    // untouched and keep serving the exact old `{"rooms":[...]}` shape. The
    // channel-bound pair rides along the same way: an old host ignores it,
    // while a new host additionally vouches that selects on this live channel
    // stay usable past the 300-second window without a fresh central grant.
    const value = strictRecord(await this.post("/api/member-connect/rooms?room_select_reuse=1&room_select_channel_bound=1", { challenge_id: challenge.challenge_id, grant_token: grantToken }, signal), "참가 중인 방");
    assertExactKeys(value, ["rooms"], "참가 중인 방", ["room_select_reuse", "room_select_channel_bound"]);
    if (!Array.isArray(value.rooms) || value.rooms.length > 50 || !value.rooms.every((room) => room && typeof room.room_id === "string" && room.room_id && typeof room.name === "string")) throw new Error("참가 중인 방을 확인하지 못했어요.");
    // A host that omits the capability keeps the per-room grant path, so the
    // reuse flag is only ever raised by a host that vouches for it.
    this.roomSelectReuse = value.room_select_reuse === true;
    this.roomSelectChannelBound = value.room_select_channel_bound === true;
    return { challenge, rooms: value.rooms as { room_id: string; name: string }[] };
  }
  private challengeLive(): boolean {
    // A select must reach the host before its challenge window closes; keep a
    // small margin so the reuse decision never races the host-side expiry.
    return this.challengeExpiresAt > Date.now() / 1000 + 5;
  }
  private async selectWithChallenge(challengeId: string, roomId: string, signal?: AbortSignal): Promise<RoomInviteJoinResponse> {
    // The channel-bound opt-in rides the select URL so the host can tell a
    // capable client past the 300-second window from an old one: old hosts
    // ignore the query, and a new host keeps the exact old wall for clients
    // that did not opt in.
    const payload = await this.post(`/api/member-connect/select?room_select_channel_bound=1`, { challenge_id: challengeId, room_id: roomId, client_id: this.clientId }, signal);
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

/**
 * Whether a session-loss re-select through this admission stays free (no
 * central grant): the host advertises room_select_channel_bound and its
 * transport is still live. Duck-typed so plain test doubles with the same
 * shape behave like the real admission; anything else is treated as paid.
 */
export function memberAdmissionFreeSelect(admission: unknown): boolean {
  if (!admission || typeof admission !== "object") return false;
  const candidate = admission as {
    transport?: { active?: boolean };
    channelBoundSelectFree?: unknown;
    roomSelectChannelBound?: unknown;
  };
  if (typeof candidate.channelBoundSelectFree === "boolean") return candidate.channelBoundSelectFree;
  if (typeof candidate.roomSelectChannelBound === "boolean") {
    return candidate.roomSelectChannelBound === true && candidate.transport?.active === true;
  }
  return false;
}
