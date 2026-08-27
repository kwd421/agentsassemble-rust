import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { TEST_WEB_CRYPTO, TestTextEncoder } from "../test/webCrypto";
import { ApiError } from "../lib/apiErrors";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import { ROOM_ADMISSION_INTENT_STORAGE_KEY } from "../lib/roomAdmissionIntent";
import { useRoomAdmission } from "./useRoomAdmission";

const deviceMocks = vi.hoisted(() => ({
  loadRememberedGuestProfile: vi.fn(() => null),
  rememberGuestProfile: vi.fn(),
}));
const apiMocks = vi.hoisted(() => ({
  joinRoomInvite: vi.fn(),
  preflightRoomInvite: vi.fn(),
  redeemOperatorPairing: vi.fn(),
}));
const surfaceMocks = vi.hoisted(() => ({
  verifyAndBindRoomSessionSurface: vi.fn().mockResolvedValue(true),
}));
const sessionStore = vi.hoisted(() => ({
  current: null as RoomGuestSession | null,
  writeError: null as Error | null,
}));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  joinRoomInvite: apiMocks.joinRoomInvite,
  preflightRoomInvite: apiMocks.preflightRoomInvite,
  redeemOperatorPairing: apiMocks.redeemOperatorPairing,
}));
vi.mock("../lib/deviceIdentity", () => deviceMocks);
vi.mock("../lib/roomDirectoryContract", async () => ({
  ...(await vi.importActual<typeof import("../lib/roomDirectoryContract")>(
    "../lib/roomDirectoryContract"
  )),
  verifyAndBindRoomSessionSurface: surfaceMocks.verifyAndBindRoomSessionSurface,
}));
vi.mock("../lib/roomGuestSession", async () => ({
  ...(await vi.importActual<typeof import("../lib/roomGuestSession")>(
    "../lib/roomGuestSession"
  )),
  persistRoomGuestSession: (session: RoomGuestSession | null) => {
    if (session && sessionStore.writeError) throw sessionStore.writeError;
    sessionStore.current = session;
  },
}));

const DEVICE_TOKEN = "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SESSION_SURFACE = {
  server_id: "11111111-1111-4111-8111-111111111111",
  authority_lineage_id: "22222222-2222-4222-8222-222222222222",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
};

function joinedPayload(sessionToken: string) {
  return {
    ...SESSION_SURFACE,
    status: "admitted",
    session_token: sessionToken,
    agent_id: "guest-1",
    display_name: "Guest",
    meeting_id: "room-1",
    invite_scope: "room",
    connection_kind: "browser",
    expires_at: "2099-01-01T00:00:00Z",
  };
}

function renderAdmission(
  onRoomJoined = vi.fn(),
  initialSession: RoomGuestSession | null = null
) {
  const hook = renderHook(() =>
    useRoomAdmission({
      deviceToken: DEVICE_TOKEN,
      clientId: "client-1",
      guestInvite: null,
      guestJoinToken: "invite-1",
      operatorPairingToken: "",
      onPairingTokenConsumed: vi.fn(),
      initialSession,
      onRoomJoined,
      onResetToLobby: vi.fn(),
    })
  );
  return { ...hook, onRoomJoined };
}

describe("room admission retry custody", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("crypto", TEST_WEB_CRYPTO);
    vi.stubGlobal("TextEncoder", TestTextEncoder);
    window.sessionStorage.clear();
    window.history.replaceState({}, "", "/join?token=invite-1");
    sessionStore.current = null;
    sessionStore.writeError = null;
    surfaceMocks.verifyAndBindRoomSessionSurface.mockResolvedValue(true);
    apiMocks.preflightRoomInvite.mockResolvedValue({
      status: "known_user",
      can_auto_join: true,
      room_id: "room-1",
      participant: {
        participant_id: "guest-1",
        display_name: "Guest",
        avatar_image_url: "",
      },
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("fails before admission when request-id storage silently refuses the write", async () => {
    const setItem = Storage.prototype.setItem;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(function (
      this: Storage,
      key,
      value
    ) {
      if (key === ROOM_ADMISSION_INTENT_STORAGE_KEY) return;
      setItem.call(this, key, value);
    });
    const { result } = renderAdmission();

    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "failed",
        code: "request_id_unavailable",
        retryable: true,
      })
    );

    expect(apiMocks.joinRoomInvite).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?token=invite-1");
    expect(window.sessionStorage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY)).toBeNull();
  });

  it("does not loop automatic join attempts after a failure", async () => {
    apiMocks.joinRoomInvite.mockRejectedValue(new Error("network unavailable"));
    const { result } = renderAdmission();

    await waitFor(() => expect(result.current.guestJoinStatus).toBe("network unavailable"));
    expect(result.current.guestJoinRetryable).toBe(true);
    await new Promise((resolve) => window.setTimeout(resolve, 20));

    expect(apiMocks.joinRoomInvite).toHaveBeenCalledOnce();
    expect(result.current.admissionState).toMatchObject({
      kind: "failed",
      operation: "join",
      retryable: true,
    });
  });

  it("retries existing-session custody through preflight without a join effect", async () => {
    const initialSession: RoomGuestSession = {
      inviteToken: "previous-invite",
      sessionToken: "existing-session",
      meetingId: "room-1",
      agentId: "guest-1",
      displayName: "Guest",
      inviteScope: "room",
      expiresAt: "2099-01-01T00:00:00Z",
      joinedAt: "2026-08-28T00:00:00Z",
      serverSurface: {
        server_id: SESSION_SURFACE.server_id,
        authority_lineage_id: SESSION_SURFACE.authority_lineage_id,
        server_product_surface: SESSION_SURFACE.server_product_surface,
      },
    };
    apiMocks.preflightRoomInvite.mockResolvedValue({
      status: "existing_session",
      can_auto_join: true,
      room_id: "room-1",
      room_label: "Room One",
      invite_scope: "room",
      participant: { participant_id: "guest-1", display_name: "Guest" },
    });
    sessionStore.current = initialSession;
    sessionStore.writeError = new Error("방 세션을 영구 저장할 수 없습니다.");
    const { result, onRoomJoined } = renderAdmission(vi.fn(), initialSession);

    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "failed",
        operation: "preflight",
        code: "session_storage_unavailable",
        retryable: true,
      })
    );
    expect(result.current.guestPreflightRetryable).toBe(true);
    expect(apiMocks.joinRoomInvite).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?token=invite-1");

    sessionStore.writeError = null;
    act(() => result.current.requestGuestJoin());
    await waitFor(() => expect(apiMocks.preflightRoomInvite).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "joined",
        source: "existing_session",
      })
    );

    expect(apiMocks.joinRoomInvite).not.toHaveBeenCalled();
    expect(onRoomJoined).toHaveBeenCalledOnce();
    expect(window.location.search).toBe("");
  });

  it("replays the frozen admission intent after an unknown join outcome", async () => {
    apiMocks.joinRoomInvite
      .mockRejectedValueOnce(new Error("network unavailable"))
      .mockResolvedValueOnce(joinedPayload("session-retried"));
    const { result } = renderAdmission();

    await waitFor(() => expect(result.current.guestJoinStatus).toBe("network unavailable"));
    const firstIntent = apiMocks.joinRoomInvite.mock.calls[0][0];
    act(() => {
      result.current.setPendingGuestDisplayName("Edited after dispatch");
      result.current.setPendingGuestAvatarImage("/api/attachments/edited?view=1");
    });
    act(() => result.current.requestGuestJoin());
    await waitFor(() =>
      expect(result.current.guestSession?.sessionToken).toBe("session-retried")
    );

    expect(apiMocks.joinRoomInvite.mock.calls[1][0]).toEqual(firstIntent);
    expect(window.sessionStorage.length).toBe(0);
  });

  it("revalidates an existing same-invite session when stale intent cleanup failed", async () => {
    const removeItem = Storage.prototype.removeItem;
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(function (
      this: Storage,
      key
    ) {
      if (key === ROOM_ADMISSION_INTENT_STORAGE_KEY) {
        throw new Error("storage unavailable");
      }
      removeItem.call(this, key);
    });
    apiMocks.joinRoomInvite.mockResolvedValue(joinedPayload("session-retained"));
    const first = renderAdmission();
    await waitFor(() =>
      expect(first.result.current.guestSession?.sessionToken).toBe("session-retained")
    );
    const retainedSession = first.result.current.guestSession;
    expect(retainedSession).not.toBeNull();
    expect(window.sessionStorage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY)).not.toBeNull();
    first.unmount();

    apiMocks.preflightRoomInvite.mockResolvedValue({
      status: "existing_session",
      can_auto_join: true,
      room_id: "room-1",
      room_label: "Room One",
      invite_scope: "room",
      participant: {
        participant_id: "guest-1",
        display_name: "Guest",
        avatar_image_url: "",
      },
      operator: false,
    });
    const second = renderAdmission(vi.fn(), retainedSession);

    await waitFor(() =>
      expect(second.result.current.admissionState).toMatchObject({
        kind: "joined",
        source: "existing_session",
      })
    );
    expect(apiMocks.preflightRoomInvite).toHaveBeenCalledTimes(2);
    expect(apiMocks.joinRoomInvite).toHaveBeenCalledOnce();
  });

  it("retires the intent only when the server proves a no-commit outcome", async () => {
    apiMocks.joinRoomInvite.mockRejectedValue(
      new ApiError(
        403,
        "The invited identity conflicts with an existing participant.",
        "participant_identity_conflict"
      )
    );
    const { result } = renderAdmission();

    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "failed",
        operation: "join",
        retryable: false,
      })
    );

    expect(apiMocks.joinRoomInvite).toHaveBeenCalledOnce();
    expect(window.sessionStorage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY)).toBeNull();
  });

  it("retains reusable retry custody when a later invite gate cannot disprove commit", async () => {
    apiMocks.joinRoomInvite
      .mockRejectedValueOnce(new Error("response lost after reusable admission"))
      .mockRejectedValueOnce(
        new ApiError(403, "Invite was revoked.", "invite_revoked")
      );
    const { result } = renderAdmission();

    await waitFor(() =>
      expect(result.current.guestJoinStatus).toBe("response lost after reusable admission")
    );
    act(() => result.current.requestGuestJoin());
    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "failed",
        operation: "join",
        code: "invite_revoked",
        retryable: false,
      })
    );

    expect(apiMocks.joinRoomInvite).toHaveBeenCalledTimes(2);
    expect(apiMocks.joinRoomInvite.mock.calls[1][0]).toEqual(
      apiMocks.joinRoomInvite.mock.calls[0][0]
    );
    expect(window.sessionStorage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY)).not.toBeNull();
  });

  it("replays a pending one-use admission directly after component replacement", async () => {
    apiMocks.joinRoomInvite
      .mockRejectedValueOnce(new Error("response lost"))
      .mockResolvedValueOnce(joinedPayload("session-after-reload"));
    const first = renderAdmission();

    await waitFor(() => expect(first.result.current.guestJoinStatus).toBe("response lost"));
    const firstIntent = apiMocks.joinRoomInvite.mock.calls[0][0];
    first.unmount();
    apiMocks.preflightRoomInvite.mockClear();

    const second = renderAdmission();
    await waitFor(() =>
      expect(second.result.current.guestSession?.sessionToken).toBe("session-after-reload")
    );

    expect(apiMocks.preflightRoomInvite).not.toHaveBeenCalled();
    expect(apiMocks.joinRoomInvite.mock.calls[1][0]).toEqual(firstIntent);
    expect(window.location.search).toBe("");
    expect(window.sessionStorage.length).toBe(0);
  });

  it("retains the exact join retry until durable session custody succeeds", async () => {
    apiMocks.joinRoomInvite.mockResolvedValue(joinedPayload("session-after-retry"));
    sessionStore.writeError = new Error("방 세션을 영구 저장할 수 없습니다.");
    const { result, onRoomJoined } = renderAdmission();

    await waitFor(() =>
      expect(result.current.admissionState).toMatchObject({
        kind: "failed",
        code: "session_storage_unavailable",
        retryable: true,
      })
    );
    const requestId = apiMocks.joinRoomInvite.mock.calls[0][0].requestId;
    expect(result.current.guestSession).toBeNull();
    expect(onRoomJoined).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?token=invite-1");
    expect(
      JSON.parse(
        window.sessionStorage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY) || "{}"
      ).requestId
    ).toBe(requestId);

    sessionStore.writeError = null;
    act(() => result.current.requestGuestJoin());
    await waitFor(() =>
      expect(result.current.guestSession?.sessionToken).toBe("session-after-retry")
    );

    expect(apiMocks.joinRoomInvite.mock.calls[1][0].requestId).toBe(requestId);
    expect(onRoomJoined).toHaveBeenCalledOnce();
    expect(window.location.search).toBe("");
    expect(window.sessionStorage.length).toBe(0);
  });
});
