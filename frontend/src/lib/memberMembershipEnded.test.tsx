import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MembershipEndedView from "../views/components/MembershipEndedView";
import { presenceStatusLabel } from "../lib/presenceStatus";
import {
  bindMembershipEndedPerson,
  isMembershipEndedError,
  isMembershipEndedTerminal,
  markMembershipEndedTerminal,
  membershipEndedCopy,
  membershipEndedScopeFromEventType,
  resetMembershipEndedTerminalForTests,
} from "./memberMembershipEnded";
import { ApiError } from "./apiErrors";
import { createFreshRoom } from "./roomDockModel";

afterEach(cleanup);
beforeEach(resetMembershipEndedTerminalForTests);

describe("membershipEnded contract", () => {
  it("maps the host refusal code and the canonical kick/export events", () => {
    expect(isMembershipEndedError(new ApiError(403, "x", "member_membership_ended"))).toBe(true);
    expect(isMembershipEndedError(new ApiError(403, "x", "other"))).toBe(false);
    expect(isMembershipEndedError(new Error("x"))).toBe(false);
    expect(membershipEndedScopeFromEventType("participant_exported")).toBe("exported");
    expect(membershipEndedScopeFromEventType("participant_kicked")).toBe("kicked");
  });

  it("copies the kicked and exported ended lines", () => {
    expect(membershipEndedCopy("모임방", "kicked")).toEqual({
      title: "방에서 내보내졌어요",
      body: "‘모임방’에 다시 참가하려면 방 관리자에게 새 초대를 받아야 해요.",
    });
    expect(membershipEndedCopy("모임방", "exported")).toEqual({
      title: "방에서 내보내졌어요",
      body: "‘모임방’에 더 이상 참가할 수 없어요.",
    });
  });

  it("keeps an ended room with the person it ended for, even after a switch in another tab", () => {
    let person = "person-a";
    bindMembershipEndedPerson(() => person);
    markMembershipEndedTerminal("s", "m");
    expect(isMembershipEndedTerminal("s", "m")).toBe(true);
    person = "person-b";
    expect(isMembershipEndedTerminal("s", "m")).toBe(false);
    person = "person-a";
    expect(isMembershipEndedTerminal("s", "m")).toBe(true);
    bindMembershipEndedPerson(() => "");
  });

  it("marks terminal rooms per host room until reset", () => {
    expect(isMembershipEndedTerminal("s", "m")).toBe(false);
    markMembershipEndedTerminal("s", "m");
    expect(isMembershipEndedTerminal("s", "m")).toBe(true);
    expect(isMembershipEndedTerminal("s", "other")).toBe(false);
    resetMembershipEndedTerminalForTests();
    expect(isMembershipEndedTerminal("s", "m")).toBe(false);
  });

  it("labels kicked presence as 내보내짐", () => {
    expect(presenceStatusLabel("kicked")).toBe("내보내짐");
  });
});

describe("MembershipEndedView", () => {
  it("shows the kicked ended copy with 확인 and no retry", () => {
    const onConfirm = vi.fn();
    const room = { ...createFreshRoom(), label: "모임방" };
    render(<MembershipEndedView room={room} scope="kicked" onConfirm={onConfirm} />);
    expect(screen.getByRole("heading", { name: "방에서 내보내졌어요" })).toBeTruthy();
    expect(screen.getByText("‘모임방’에 다시 참가하려면 방 관리자에게 새 초대를 받아야 해요.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "다시 연결" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "확인" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("shows the exported variant without a retry button", () => {
    const room = { ...createFreshRoom(), label: "모임방" };
    render(<MembershipEndedView room={room} scope="exported" onConfirm={() => undefined} />);
    expect(screen.getByText("‘모임방’에 더 이상 참가할 수 없어요.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "다시 연결" })).toBeNull();
  });
});

describe("selectMemberRoomOnDemand membership-ended refusal", () => {
  it("parks the room as terminal with the ended reason and never reuses the cached bearer", async () => {
    const { selectMemberRoomOnDemand } = await import("../app/useMemberRoomAutoReselect");
    const refusal = new ApiError(403, "참가를 마치지 못했어요.", "member_membership_ended");
    const selectRoom = vi.fn().mockRejectedValue(refusal);
    let error: { roomId: string; message: string; membershipEnded?: "kicked" | "exported" } | null = null;
    await selectMemberRoomOnDemand({
      roomId: "rail-1",
      meetingId: "room-1",
      memberServerId: "server-1",
      liveSessionFor: () => null,
      selectRoom: selectRoom as never,
      acceptSession: (async () => true) as never,
      setSelectError: ((update: unknown) => {
        if (typeof update === "function") {
          error = (update as (c: null) => typeof error)(null);
        } else {
          error = update as typeof error;
        }
      }) as never,
    });
    expect(error).toEqual({
      roomId: "rail-1",
      message: "방을 열지 못했어요. 다시 시도해 주세요.",
      membershipEnded: "kicked",
    });
    expect(isMembershipEndedTerminal("server-1", "room-1")).toBe(true);
    // A terminal room never reinstalls its cached bearer: the next tap goes
    // straight to a fresh select instead.
    const acceptSession = vi.fn(async () => true);
    const fresh = vi.fn(async () => ({ meeting_id: "room-1", server_id: "server-1" }));
    let second: unknown = "unset";
    await selectMemberRoomOnDemand({
      roomId: "rail-1",
      meetingId: "room-1",
      memberServerId: "server-1",
      liveSessionFor: () => ({ session_token: "cached", expires_at: new Date(Date.now() + 3600_000).toISOString() }),
      selectRoom: fresh as never,
      acceptSession: acceptSession as never,
      setSelectError: ((update: unknown) => {
        second = typeof update === "function" ? (update as (c: unknown) => unknown)({ roomId: "rail-1" }) : update;
      }) as never,
    });
    expect(fresh).toHaveBeenCalledOnce();
    expect(acceptSession).toHaveBeenCalledOnce();
    expect(second).toBeNull();
  });
});

describe("ended rooms belong to the signed-in person", () => {
  it("forgets every ended room on sign-out and one room on re-admission", async () => {
    const ended = await import("./memberMembershipEnded");
    const { clearCentralSession } = await import("./central/identity");
    ended.markMembershipEndedTerminal("server-s", "room-m");
    ended.markMembershipEndedTerminal("server-s", "room-n");
    ended.clearMembershipEndedTerminal("server-s", "room-m");
    expect(ended.isMembershipEndedTerminal("server-s", "room-m")).toBe(false);
    expect(ended.isMembershipEndedTerminal("server-s", "room-n")).toBe(true);
    clearCentralSession();
    expect(ended.isMembershipEndedTerminal("server-s", "room-n")).toBe(false);
  });
});
