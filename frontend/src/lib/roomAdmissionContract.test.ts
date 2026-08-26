import { describe, expect, it } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import {
  parseGuestRecoveryRedeemResponse,
  parseOperatorPairingRedeemResponse,
  parseRoomInviteAdmissionResponse,
  parseRoomInviteJoinResponse,
} from "./roomAdmissionContract";

const surface = {
  server_id: "11111111-1111-4111-8111-111111111111",
  authority_lineage_id: "22222222-2222-4222-8222-222222222222",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
};

const common = {
  ...surface,
  session_token: "aas1.session",
  agent_id: "guest-1",
  display_name: "Guest",
  meeting_id: "room-1",
  invite_scope: "room",
  participant_type: "human",
  client_type: "browser",
  provider_kind: "manual",
  connection_kind: "native_remote_room_client",
  expires_at: "2099-01-01T00:00:00Z",
  room_label: "Room One",
  room_topic: "",
  room_created_at: "2026-07-10T00:00:00Z",
};

const join = {
  ...common,
  status: "admitted",
  request_id: "123e4567-e89b-42d3-a456-426614174000",
  owner_display_name: "",
  owner_id: "owner-1",
  stable_identity: false,
  operator: false,
  client_id: "client-1",
  guide: {
    welcome: "Welcome",
    how_to: ["Read and send messages."],
    etiquette: [],
    session: { expires_in_seconds: 3600, rejoin: "Request another invite." },
  },
};

describe("room admission response contracts", () => {
  it("accepts only exact preflight variants without defaulting room authority", () => {
    expect(
      parseRoomInviteAdmissionResponse({
        status: "profile_required",
        can_auto_join: false,
        room_id: "room-1",
        room_label: "Room One",
        invite_scope: "read_only",
      })
    ).toMatchObject({ room_id: "room-1", invite_scope: "read_only" });
    expect(() =>
      parseRoomInviteAdmissionResponse({
        status: "profile_required",
        can_auto_join: false,
        room_id: "room-1",
        room_label: "Room One",
      })
    ).toThrow(/계약/);
    expect(() =>
      parseRoomInviteAdmissionResponse({
        status: "invite_invalid",
        reason: "invite_invalid",
        can_auto_join: false,
        ignored: true,
      })
    ).toThrow(/계약/);
  });

  it("binds an exact invite response to the request and preflight room", () => {
    expect(
      parseRoomInviteJoinResponse(join, join.request_id, "room-1", "client-1")
    ).toMatchObject({
      request_id: join.request_id,
      meeting_id: "room-1",
      invite_scope: "room",
    });
    expect(() =>
      parseRoomInviteJoinResponse(
        { ...join, invite_scope: undefined },
        join.request_id,
        "room-1",
        "client-1"
      )
    ).toThrow(/invite_scope/);
    expect(() =>
      parseRoomInviteJoinResponse(
        { ...join, unexpected: true },
        join.request_id,
        "room-1",
        "client-1"
      )
    ).toThrow(/계약/);
    expect(() =>
      parseRoomInviteJoinResponse(join, join.request_id, "room-2", "client-1")
    ).toThrow(/확인된 방/);
    expect(() =>
      parseRoomInviteJoinResponse(join, join.request_id, "room-1", "client-2")
    ).toThrow(/현재 클라이언트/);
  });

  it("accepts only the canonical operator pairing shape", () => {
    const pairing = {
      ...common,
      status: "admitted",
      owner_id: "operator-local-user",
      stable_identity: true,
      operator: true,
    };
    expect(parseOperatorPairingRedeemResponse(pairing).operator).toBe(true);
    expect(() =>
      parseOperatorPairingRedeemResponse({ ...pairing, operator: false })
    ).toThrow(/운영자 연결 신원/);
  });

  it("binds recovery to the requested room and client", () => {
    const recovery = {
      ...common,
      status: "recovered",
      client_id: "client-1",
      room_uid: "33333333-3333-4333-8333-333333333333",
      joined_at: "2026-07-11T00:00:00Z",
      recovery_code: "replacement-code",
    };
    expect(
      parseGuestRecoveryRedeemResponse(recovery, "room-1", "client-1")
    ).toMatchObject({ meeting_id: "room-1", client_id: "client-1" });
    expect(() =>
      parseGuestRecoveryRedeemResponse(recovery, "room-1", "client-2")
    ).toThrow(/현재 클라이언트/);
  });
});
