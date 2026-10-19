import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberRoomAutoReselect } from "./useMemberRoomAutoReselect";
import { useMemberRoomLive } from "./useMemberRoomLive";
import type { RoomDockItem } from "../lib/roomDockModel";
import {
  MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP,
  MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE,
  memberRoomAutoReselectCount,
  noteMemberHostExternalReselectAttach,
  resetMemberRoomAutoReselectBudgetForTests,
  tryClaimMemberRoomAutoReselect,
} from "../lib/central/memberRoomAutoReselectBudget";
import { ApiError } from "../lib/apiErrors";
import { resetMembershipEndedTerminalForTests } from "../lib/memberMembershipEnded";

const ACCOUNT = "person";
const SERVER = "friend-server";
const MEETING = "friend-room";

function room(): RoomDockItem {
  return {
    id: "room-1",
    label: "Friend room",
    meetingId: MEETING,
    serverId: SERVER,
    roomOrigin: "remote_server",
    connectionState: "remote",
    topic: "",
    shortLabel: "F",
    icon: (() => null) as unknown as RoomDockItem["icon"],
    createdAt: "",
    tone: "fresh",
  } as unknown as RoomDockItem;
}

function capacityError(): ApiError {
  return new ApiError(429, "room_admission_capacity_reached", "방이 꽉 찼어요.");
}

describe("memberRoomAutoReselectBudget", () => {
  beforeEach(() => {
    resetMemberRoomAutoReselectBudgetForTests();
    resetMembershipEndedTerminalForTests();
  });

  it("allows one automatic attempt per external re-attach up to a daily cap", () => {
    // No external attach yet: first claim is for generation 0.
    expect(tryClaimMemberRoomAutoReselect(ACCOUNT, SERVER, MEETING)).toEqual({ allowed: true });
    // Same generation (internal re-attach inside selectRoom) never releases
    // another immediate attempt.
    expect(tryClaimMemberRoomAutoReselect(ACCOUNT, SERVER, MEETING)).toEqual({
      allowed: false,
      reason: "already-attempted",
    });
    // Each slow-schedule slot bumps the external generation and releases
    // exactly one more attempt until the hard daily cap.
    for (let i = 1; i < MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP; i += 1) {
      noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
      expect(tryClaimMemberRoomAutoReselect(ACCOUNT, SERVER, MEETING)).toEqual({ allowed: true });
    }
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    expect(tryClaimMemberRoomAutoReselect(ACCOUNT, SERVER, MEETING)).toEqual({
      allowed: false,
      reason: "exhausted",
    });
    expect(memberRoomAutoReselectCount(ACCOUNT, SERVER, MEETING)).toBe(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP,
    );
  });
});

describe("useMemberRoomAutoReselect bounded loop (fake timers)", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    resetMemberRoomAutoReselectBudgetForTests();
    resetMembershipEndedTerminalForTests();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    resetMemberRoomAutoReselectBudgetForTests();
    resetMembershipEndedTerminalForTests();
  });

  function setupLoopHarness() {
    // The first external attach (initial connect) releases generation 1.
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    let admission: Record<string, unknown> = { id: "admission-0", transport: { active: true } };
    const admissionFor = vi.fn(() => admission);
    let grants = 0;
    const selectRoom = vi.fn(async () => {
      grants += 1;
      // Failure path internally re-attaches WITHOUT bumping the external
      // generation: fresh admission object, same generation.
      admission = { id: `admission-internal-${grants}`, transport: { active: true } };
      throw capacityError();
    });
    const acceptSession = vi.fn(async () => true);
    let selectError: { roomId: string; message: string } | null = null;
    const setSelectError = vi.fn((update: unknown) => {
      if (typeof update === "function") {
        selectError = (update as (current: unknown) => never)(selectError) as never;
      } else {
        selectError = update as never;
      }
    });
    const snapshots = [{ serverId: SERVER, phase: "connected", origin: "https://friend.test" }];
    const hook = renderHook(
      ({ snap }: { snap: typeof snapshots }) =>
        useMemberRoomAutoReselect({
          activeRoom: room(),
          ownerServerId: undefined,
          accountId: ACCOUNT,
          hasAnchor: () => true,
          isRefused: () => false,
          snapshots: snap as never,
          admissionFor: admissionFor as never,
          guestSession: null,
          guestExpired: false,
          admittedSessionToken: "",
          liveSessionFor: () => null,
          selectRoom: selectRoom as never,
          acceptSession: acceptSession as never,
          setSelectError: setSelectError as never,
        }),
      { initialProps: { snap: snapshots } },
    );
    return {
      ...hook,
      admissionFor,
      selectRoom,
      setSelectError,
      getError: () => selectError,
      getGrants: () => grants,
      replaceAdmissionExternal: () => {
        noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
        admission = { id: `admission-external-${Date.now()}-${Math.random()}`, transport: { active: true } };
      },
    };
  }

  it("capacity 429 on every select stays within the cap over 6 simulated hours", async () => {
    const harness = setupLoopHarness();
    // First automatic attempt runs on the fresh external admission.
    await waitFor(() => expect(harness.selectRoom).toHaveBeenCalledTimes(1));
    expect(harness.getError()?.message).toBe(MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE);
    const grantsAfterFirst = harness.getGrants();

    // Simulate 6 hours of slow-reconnect slots + internal re-attach churn:
    // every slot is a new EXTERNAL admission, each select failure swaps in an
    // INTERNAL admission that must not retrigger. Advance fake timers to cover
    // the backoff cadence; the count must stay bounded by the daily cap.
    for (let slot = 0; slot < 30; slot += 1) {
      harness.replaceAdmissionExternal();
      harness.rerender({ snap: [{ serverId: SERVER, phase: "connected", origin: "https://friend.test" }] });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(12 * 60_000);
      });
    }
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6 * 60_60_000);
    });
    expect(harness.selectRoom.mock.calls.length).toBeLessThanOrEqual(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP,
    );
    expect(harness.getGrants() - grantsAfterFirst).toBeLessThanOrEqual(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP,
    );
    // Total central grants for automatic attempts stay bounded: at most one
    // grant per automatic attempt (cap * 1), far below the ~80 that would
    // exhaust the 240-unit daily allowance.
    expect(harness.getGrants()).toBeLessThanOrEqual(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP);
    // The user is told to press reconnect, never left in silence.
    expect(harness.getError()?.message).toBe(MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE);
  });

  it("remount does not reset the budget", async () => {
    const first = setupLoopHarness();
    await waitFor(() => expect(first.selectRoom).toHaveBeenCalledTimes(1));
    // Exhaust the daily cap through slow slots.
    for (let i = 1; i < MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP; i += 1) {
      first.replaceAdmissionExternal();
      first.rerender({ snap: [{ serverId: SERVER, phase: "connected", origin: "https://friend.test" }] });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
    }
    await waitFor(() =>
      expect(first.selectRoom.mock.calls.length).toBe(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP),
    );
    const callsBeforeRemount = first.selectRoom.mock.calls.length;
    first.unmount();

    // Remount with the same account+room and a fresh external admission:
    // the persisted per-account budget still blocks further automatics.
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    const admission = { id: "admission-remount", transport: { active: true } };
    const selectRoom = vi.fn(async () => {
      throw capacityError();
    });
    let selectError: { roomId: string; message: string } | null = null;
    const { result } = renderHook(() =>
      useMemberRoomAutoReselect({
        activeRoom: room(),
        ownerServerId: undefined,
        accountId: ACCOUNT,
        hasAnchor: () => true,
        isRefused: () => false,
        snapshots: [{ serverId: SERVER, phase: "connected", origin: "https://friend.test" }] as never,
        admissionFor: (() => admission) as never,
        guestSession: null,
        guestExpired: false,
        admittedSessionToken: "",
        liveSessionFor: () => null,
        selectRoom: selectRoom as never,
        acceptSession: (async () => true) as never,
        setSelectError: ((update: unknown) => {
          selectError = typeof update === "function" ? (update as (c: unknown) => never)(selectError) as never : (update as never);
        }) as never,
      }),
    );
    expect(result).toBeTruthy();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(selectRoom).not.toHaveBeenCalled();
    expect(callsBeforeRemount).toBe(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP);
    // Remount parks too, but the user still sees the reconnect line.
    expect(selectError).toEqual({ roomId: "room-1", message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
  });

  it("explicit room open still tries once immediately when automatics are parked", async () => {
    const harness = setupLoopHarness();
    await waitFor(() => expect(harness.selectRoom).toHaveBeenCalledTimes(1));
    // Park the automatics.
    for (let i = 1; i < MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP; i += 1) {
      harness.replaceAdmissionExternal();
      harness.rerender({ snap: [{ serverId: SERVER, phase: "connected", origin: "https://friend.test" }] });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
    }
    await waitFor(() =>
      expect(harness.selectRoom.mock.calls.length).toBe(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP),
    );
    // Explicit user action bypasses the automatic budget: a manual tap goes
    // through selectRoom directly exactly once.
    const manualSelect = vi.fn(async () => ({ meeting_id: MEETING }));
    const manualAccept = vi.fn(async () => true);
    const { selectMemberRoomOnDemand } = await import("./useMemberRoomAutoReselect");
    let manualError: unknown = null;
    await selectMemberRoomOnDemand({
      roomId: "room-1",
      meetingId: MEETING,
      memberServerId: SERVER,
      liveSessionFor: () => null,
      selectRoom: manualSelect as never,
      acceptSession: manualAccept as never,
      setSelectError: ((update: unknown) => {
        manualError = typeof update === "function" ? (update as (c: unknown) => unknown)(manualError) : update;
      }) as never,
    });
    expect(manualSelect).toHaveBeenCalledTimes(1);
    expect(manualError).toBeNull();
  });

  it("success path unchanged: host restart -> one reselect -> connected", async () => {
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    let admission: Record<string, unknown> = { id: "admission-old", transport: { active: true } };
    const selectRoom = vi.fn(async () => ({ meeting_id: MEETING, server_id: SERVER }));
    const acceptSession = vi.fn(async () => true);
    let selectError: { roomId: string; message: string } | null = { roomId: "room-1", message: "stale" };
    const hook = renderHook(
      ({ snap, adm }: { snap: unknown[]; adm: unknown }) =>
        useMemberRoomAutoReselect({
          activeRoom: room(),
          ownerServerId: undefined,
          accountId: ACCOUNT,
          hasAnchor: () => true,
          isRefused: () => false,
          snapshots: snap as never,
          admissionFor: (() => adm) as never,
          guestSession: null,
          guestExpired: false,
          admittedSessionToken: "",
          liveSessionFor: () => null,
          selectRoom: selectRoom as never,
          acceptSession: acceptSession as never,
          setSelectError: ((update: unknown) => {
            selectError =
              typeof update === "function"
                ? (update as (c: unknown) => never)(selectError) as never
                : (update as never);
          }) as never,
        }),
      {
        initialProps: {
          snap: [{ serverId: SERVER, phase: "connected", origin: "https://old.test" }],
          adm: admission,
        },
      },
    );
    await waitFor(() => expect(selectRoom).toHaveBeenCalledTimes(1));
    // Host restarts on a new origin: one external re-attach, one reselect.
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    admission = { id: "admission-new", transport: { active: true } };
    hook.rerender({
      snap: [{ serverId: SERVER, phase: "connected", origin: "https://new.test" }],
      adm: admission,
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    await waitFor(() => expect(selectRoom).toHaveBeenCalledTimes(2));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    // No spin: exactly one reselect for the restart.
    expect(selectRoom).toHaveBeenCalledTimes(2);
    expect(selectError).toBeNull();
  });
});

describe("useMemberRoomLive background budget (fake timers)", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    resetMemberRoomAutoReselectBudgetForTests();
    resetMembershipEndedTerminalForTests();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    resetMemberRoomAutoReselectBudgetForTests();
    resetMembershipEndedTerminalForTests();
  });

  it("background re-selects on admission change obey the same daily cap", async () => {
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    const selectBackgroundRoom = vi.fn(async (roomId: string) => ({
      session_token: `session-${roomId}`,
      agent_id: "member-1",
      display_name: "Member",
      meeting_id: roomId,
      server_id: SERVER,
      server_product_surface: { websocket_streams: ["room_events"] },
    }));
    let admission: Record<string, unknown> = {
      transport: { active: true },
      deviceToken: "device",
      selectBackgroundRoom,
    };
    const admissionFor = vi.fn(() => admission);
    const openSocket = vi.fn(() => ({ close: vi.fn() }));
    const snapshots = [{
      serverId: SERVER,
      phase: "connected",
      origin: "https://friend.test",
      rooms: [{ room_id: MEETING, name: MEETING }],
      detail: null,
    }];
    const hook = renderHook(
      ({ snap }: { snap: typeof snapshots }) =>
        useMemberRoomLive({
          snapshots: snap as never,
          admissionFor: admissionFor as never,
          activeServerId: "",
          activeMeetingId: "",
          accountId: ACCOUNT,
          openSocket: openSocket as never,
        }),
      { initialProps: { snap: snapshots } },
    );
    await waitFor(() => expect(selectBackgroundRoom).toHaveBeenCalledTimes(1));

    // Exhaust the shared per-room budget through external re-attaches.
    for (let i = 1; i < MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP; i += 1) {
      noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
      admission = { transport: { active: true }, deviceToken: "device", selectBackgroundRoom };
      admissionFor.mockReturnValue(admission);
      hook.rerender({
        snap: [{
          serverId: SERVER,
          phase: "connected",
          origin: `https://friend-${i}.test`,
          rooms: [{ room_id: MEETING, name: MEETING }],
          detail: null,
        }],
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
    }
    await waitFor(() =>
      expect(selectBackgroundRoom.mock.calls.length).toBe(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP),
    );
    const parkedCalls = selectBackgroundRoom.mock.calls.length;

    // One more slow-slot admission: the shared budget parks, no grant spent.
    noteMemberHostExternalReselectAttach(ACCOUNT, SERVER);
    admission = { transport: { active: true }, deviceToken: "device", selectBackgroundRoom };
    admissionFor.mockReturnValue(admission);
    hook.rerender({
      snap: [{
        serverId: SERVER,
        phase: "connected",
        origin: "https://friend-parked.test",
        rooms: [{ room_id: MEETING, name: MEETING }],
        detail: null,
      }],
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6 * 60_60_000);
    });
    expect(selectBackgroundRoom.mock.calls.length).toBe(parkedCalls);
  });
});
