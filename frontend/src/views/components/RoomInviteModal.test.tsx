import "../../test/nativeDialog";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HumanInvitePresentation } from "../../app/useManagedHumanInvites";
import type { OperatorPairingPresentation } from "../../app/useManagedOperatorPairings";
import RoomInviteModal from "./RoomInviteModal";
const friendsApi = vi.hoisted(() => ({ list: vi.fn().mockResolvedValue([]) }));
vi.mock("../../api/friends", () => ({ fetchSavedFriends: friendsApi.list }));

afterEach(cleanup);

it("uses a saved human name for human admission while excluding AI contacts", async () => {
  const details = { display_name: "초대 친구", handle: "friend", participant_type: "human", provider_kind: "", connection_kind: "", agent_id: "", source_agent_id: "", last_meeting_id: "", status: "offline", source: "manual", last_seen_at: null };
  const friend = { friend_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", revision: 1, details, created_at: "2026-09-08T00:00:00Z", updated_at: "2026-09-08T00:00:00Z" };
  friendsApi.list.mockResolvedValueOnce([friend, { ...friend, friend_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", details: { ...details, participant_type: "remote", display_name: "외부 에이전트" } }]);
  const { onGenerateSecureInvite } = renderInviteModal();
  const select = screen.getByLabelText("사람 친구 초대");
  await waitFor(() => expect((select as HTMLSelectElement).disabled).toBe(false));
  expect(screen.queryByRole("option", { name: "외부 에이전트 · friend" })).toBeNull();
  fireEvent.change(select, { target: { value: friend.friend_id } });
  fireEvent.click(screen.getByRole("button", { name: "사람 초대 링크 생성" }));
  expect(onGenerateSecureInvite).toHaveBeenCalledWith({ maxUses: 1, ttlSeconds: 86400, displayName: "초대 친구" }, false);
});

function renderInviteModal({
  publicAccess = true,
  activeWithoutUrl = false,
  phase,
  requestState = "idle",
  humanInvites = [],
  pairingAvailable = false,
  operatorPairings = [],
}: {
  publicAccess?: boolean;
  activeWithoutUrl?: boolean;
  phase?: "stopped" | "starting" | "running" | "stopping" | "error";
  requestState?: "idle" | "starting" | "stopping";
  humanInvites?: HumanInvitePresentation[];
  pairingAvailable?: boolean;
  operatorPairings?: OperatorPairingPresentation[];
} = {}) {
  const onGenerateSecureInvite = vi.fn();
  const onCreatePairing = vi.fn();
  const onCopyPairing = vi.fn();
  const onRevokePairing = vi.fn();
  const onStopTunnel = vi.fn();
  const onCopyHumanInvite = vi.fn();
  const onRevokeHumanInvite = vi.fn();
  const tunnelPhase = phase || (publicAccess || activeWithoutUrl ? "running" : "stopped");
  const tunnelActive = ["starting", "running", "stopping"].includes(tunnelPhase);
  render(
    <RoomInviteModal
      roomLabel="제품 방"
      humanInvites={humanInvites}
      operatorPairings={operatorPairings}
      onCreatePairing={pairingAvailable ? onCreatePairing : undefined}
      onCopyPairing={pairingAvailable ? onCopyPairing : undefined}
      onRevokePairing={pairingAvailable ? onRevokePairing : undefined}
      publicUrl={publicAccess ? "https://room.example.com" : ""}
      publicAccessTransition={requestState}
      tunnelStatus={{
        available: true,
        running: tunnelActive,
        phase: tunnelPhase,
        public_url: publicAccess ? "https://room.example.com" : "",
        local_url: "http://127.0.0.1:43123",
        stable_phase: "unconfigured",
      }}
      onClose={vi.fn()}
      onGenerateSecureInvite={onGenerateSecureInvite}
      onCopyHumanInvite={onCopyHumanInvite}
      onRevokeHumanInvite={onRevokeHumanInvite}
      onStartTunnel={vi.fn()}
      onStopTunnel={onStopTunnel}
    />
  );
  return {
    onGenerateSecureInvite, onCreatePairing, onCopyPairing, onRevokePairing,
    onStopTunnel,
    onCopyHumanInvite,
    onRevokeHumanInvite,
  };
}

describe("RoomInviteModal", () => {
  it("does not expose the removed client-only preview entrance", () => {
    renderInviteModal();

    expect(screen.queryByText(/로컬\/dev 미리보기/)).toBeNull();
  });

  it("does not present an active tunnel without a trusted public URL as open", () => {
    const { onStopTunnel } = renderInviteModal({
      publicAccess: false,
      activeWithoutUrl: true,
    });

    expect(screen.getByText("외부 접속 꺼짐")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "외부 접속 열기" }) as HTMLButtonElement)
        .disabled
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "외부 접속 끄기" }));
    expect(onStopTunnel).toHaveBeenCalledOnce();
  });

  it("keeps Stop available when the server remains in its starting phase", () => {
    const { onStopTunnel } = renderInviteModal({
      publicAccess: false,
      phase: "starting",
      requestState: "starting",
    });

    expect(screen.getByText("공개 준비 중")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "외부 접속 열기" }) as HTMLButtonElement)
        .disabled
    ).toBe(true);
    const stop = screen.getByRole("button", { name: "외부 접속 끄기" });
    expect((stop as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(stop);
    expect(onStopTunnel).toHaveBeenCalledOnce();
  });

  it("keeps Stop available while Start still awaits its first server response", () => {
    const { onStopTunnel } = renderInviteModal({
      publicAccess: false,
      phase: "stopped",
      requestState: "starting",
    });

    const stop = screen.getByRole("button", { name: "외부 접속 끄기" });
    expect((stop as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(stop);
    expect(onStopTunnel).toHaveBeenCalledOnce();
  });

  it("creates a human invite with the selected use limit and lifetime", () => {
    const { onGenerateSecureInvite } = renderInviteModal();

    fireEvent.change(screen.getByLabelText("초대 가능 인원"), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText("링크 유효시간"), {
      target: { value: "604800" },
    });
    fireEvent.click(screen.getByRole("button", { name: "사람 초대 링크 생성" }));

    expect(onGenerateSecureInvite).toHaveBeenCalledWith(
      { maxUses: 5, ttlSeconds: 604800 },
      false
    );
  });

  it("presents retained human invite custody without reopening copy eligibility", () => {
    const current: HumanInvitePresentation = {
      key: "current",
      displayName: "Guest",
      maxUses: 1,
      ttlSeconds: 86400,
      expiresAt: "2026-08-29T00:00:00+00:00",
      expired: false,
      retired: false,
      originCurrent: true,
      authorityCurrent: true,
      revocation: "idle",
      copyUrl: "https://room.example.com/join?token=aaj1_current",
    };
    const uncertain: HumanInvitePresentation = {
      ...current,
      key: "uncertain",
      retired: true,
      revocation: "unknown",
      copyUrl: "",
    };
    const dead: HumanInvitePresentation = {
      ...current,
      key: "dead",
      retired: true,
      revocation: "dead",
      copyUrl: "",
    };
    const { onCopyHumanInvite, onRevokeHumanInvite } = renderInviteModal({
      humanInvites: [current, uncertain, dead],
    });

    expect((screen.getByLabelText("사람 초대 링크") as HTMLInputElement).value).toBe(
      "보안 초대 링크 발급됨"
    );
    expect(screen.queryByDisplayValue(current.copyUrl)).toBeNull();
    expect(document.body.innerHTML).not.toContain("aaj1_current");
    expect(screen.getByText(/복사 가능$/)).toBeTruthy();
    expect(screen.getByText(/폐기 결과 미확인$/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "사람 초대 1 링크 복사" }));
    fireEvent.click(screen.getByRole("button", { name: "사람 초대 2 폐기" }));
    expect(onCopyHumanInvite).toHaveBeenCalledWith("current");
    expect(onRevokeHumanInvite).toHaveBeenCalledWith("uncertain");
    expect(
      (screen.getByRole("button", { name: "사람 초대 2 링크 복사" }) as HTMLButtonElement)
        .disabled
    ).toBe(true);
    const deadRevoke = screen.getByRole("button", { name: "사람 초대 3 폐기" });
    expect((deadRevoke as HTMLButtonElement).disabled).toBe(true);
    expect(deadRevoke.textContent).toBe("폐기됨");

    fireEvent.change(screen.getByLabelText("초대 가능 인원"), {
      target: { value: "5" },
    });
    expect((screen.getByLabelText("사람 초대 링크") as HTMLInputElement).value).toBe("");
    expect(
      (screen.getByRole("button", { name: "사람 초대 1 링크 복사" }) as HTMLButtonElement)
        .disabled
    ).toBe(false);
  });

  it("does not expose an external AI invite before its Room Connector owner exists", () => {
    renderInviteModal();

    expect(screen.queryByRole("heading", { name: "외부 AI 세션 초대" })).toBeNull();
    expect(screen.queryByText(/Room Connector 설치/)).toBeNull();
  });

  it("connects pairing creation and keeps uncertain revocation out of clipboard actions", () => {
    const { onCreatePairing, onRevokePairing } = renderInviteModal({
      pairingAvailable: true,
      operatorPairings: [{ key: "pairing-one", expiresAt: "2099-01-01T00:00:00Z",
        state: "unknown", copyable: false, expired: false }],
    });
    fireEvent.click(screen.getByRole("button", { name: "운영자 기기 연결 링크 생성" }));
    expect(onCreatePairing).toHaveBeenCalledOnce();
    expect((screen.getByRole("button", { name: "기기 연결 1 링크 복사" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "기기 연결 1 해제" }));
    expect(onRevokePairing).toHaveBeenCalledWith("pairing-one");
  });

  it("requires the operator-pairing issuer before offering device connection", () => {
    renderInviteModal();

    expect(screen.queryByText("고급 연결 설정")).toBeNull();
    expect(screen.queryByRole("button", { name: "운영자 기기 연결 링크 생성" })).toBeNull();
  });
});
