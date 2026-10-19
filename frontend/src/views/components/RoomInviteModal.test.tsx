import "../../test/nativeDialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  const select = await screen.findByLabelText("받는 사람");
  await waitFor(() => expect((select as HTMLSelectElement).disabled).toBe(false));
  expect(screen.queryByRole("option", { name: "외부 에이전트 · friend" })).toBeNull();
  fireEvent.change(select, { target: { value: friend.friend_id } });
  fireEvent.click(screen.getByRole("button", { name: "사람 초대 링크 생성" }));
  expect(onGenerateSecureInvite).toHaveBeenCalledWith({ maxUses: 1, ttlSeconds: 86400, displayName: "초대 친구" }, false);
});

function renderInviteModal({
  publicAccess = true,
  query = "confirmed",
  activeWithoutUrl = false,
  phase,
  lastError,
  requestState = "idle",
  humanInvites = [],
  pairingAvailable = false,
  operatorPairings = [],
  copyStatus,
  quotaParkedUntil = null,
  friendRoom = false,
  remoteOwner = false,
}: {
  lastError?: string;
  publicAccess?: boolean;
  query?: "checking" | "confirmed" | "unavailable";
  activeWithoutUrl?: boolean;
  phase?: "stopped" | "starting" | "running" | "stopping" | "error";
  requestState?: "idle" | "starting" | "stopping";
  humanInvites?: HumanInvitePresentation[];
  pairingAvailable?: boolean;
  operatorPairings?: OperatorPairingPresentation[];
  copyStatus?: string;
  quotaParkedUntil?: number | null;
  friendRoom?: boolean;
  remoteOwner?: boolean;
} = {}) {
  const retry = vi.fn();
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
      publicAccessQuery={query}
      onRetryPublicAccess={retry}
      tunnelStatus={{
        last_error: lastError,
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
      copyStatus={copyStatus}
      quotaParkedUntil={quotaParkedUntil}
      canControlIngress={!friendRoom && !remoteOwner}
      showHosting={!friendRoom && !remoteOwner}
      canCreateLink={!friendRoom}
      friendAuthority={friendRoom ? null : undefined}
    />
  );
  return {
    onGenerateSecureInvite, onCreatePairing, onCopyPairing, onRevokePairing,
    retry, onStopTunnel,
    onCopyHumanInvite,
    onRevokeHumanInvite,
  };
}

describe("RoomInviteModal", () => {
  it("keeps this computer's external access out of a friend's room invite", () => {
    friendsApi.list.mockClear();
    renderInviteModal({ publicAccess: false, friendRoom: true });
    expect(screen.queryByRole("button", { name: "외부 접속 열기" })).toBeNull();
    expect(screen.queryByRole("radio", { name: "내 기기" })).toBeNull();
    expect(screen.getByText("초대 링크는 방 주인만 만들 수 있어요.")).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: "사람 초대 링크" })).toBeNull();
    // No own friends list here: nothing is fetched, no picker or error shows.
    expect(friendsApi.list).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "다시 읽기" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("waits for the host's own external access when it cannot open it from here", () => {
    const { onGenerateSecureInvite } = renderInviteModal({ publicAccess: false, remoteOwner: true });
    expect(screen.getByRole("textbox", { name: "사람 초대 링크" }).getAttribute("placeholder")).toBe("서버 주인이 외부 접속을 열면 링크를 만들 수 있어요");
    const create = screen.getByRole("button", { name: "사람 초대 링크 생성" }) as HTMLButtonElement;
    expect(create.disabled).toBe(true);
    fireEvent.click(create);
    expect(onGenerateSecureInvite).not.toHaveBeenCalled();
  });

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
    expect(screen.queryByRole("button", { name: "외부 접속 열기" })).toBeNull();
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
    expect(screen.queryByRole("button", { name: "외부 접속 열기" })).toBeNull();
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

    fireEvent.click(screen.getByRole("button", { name: "링크 설정" }));
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
      "초대 링크가 준비됐어요"
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

    fireEvent.click(screen.getByRole("button", { name: "링크 설정" }));
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
    fireEvent.click(screen.getByRole("tab", { name: "내 기기" }));
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

it.each(["checking", "unavailable"] as const)("does not label %s ingress as off", query => {
  const { retry } = renderInviteModal({ publicAccess: false, query });
  expect(screen.queryByText("외부 접속 꺼짐")).toBeNull();
  expect(screen.queryByRole("button", { name: "외부 접속 열기" })).toBeNull();
  expect((screen.getByRole("button", { name: "사람 초대 링크 생성" }) as HTMLButtonElement).disabled).toBe(true);
  if (query === "unavailable") {
    fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
    expect(retry).toHaveBeenCalledOnce();
  } else expect(screen.getByText("외부 접속 확인 중")).toBeTruthy();
});

it("renders a local QR for a usable device link", () => {
  renderInviteModal({ pairingAvailable: true, operatorPairings: [{ key: "qr", expiresAt: "2099-01-01T00:00:00Z", state: "ready", copyable: true, expired: false, qrUrl: "https://host.example/pair?token=test" }] });
  fireEvent.click(screen.getByRole("tab", { name: "내 기기" }));
  expect(screen.getByRole("img", { name: "기기 연결 1 QR 코드" }).tagName.toLowerCase()).toBe("svg");
});

it.each(["unknown", "revoking", "revoked"] as const)("hides a QR in %s state", state => {
  renderInviteModal({ pairingAvailable: true, operatorPairings: [{ key: "qr", expiresAt: "2099-01-01T00:00:00Z", state, copyable: false, expired: false, qrUrl: "https://host.example/pair?token=test" }] });
  fireEvent.click(screen.getByRole("tab", { name: "내 기기" }));
  expect(screen.queryByRole("img", { name: "기기 연결 1 QR 코드" })).toBeNull();
});

it("shows the copy button's own copied state and returns without a success box", async () => {
  vi.useFakeTimers();
  try {
    const invite: HumanInvitePresentation = {
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
    const { onCopyHumanInvite } = renderInviteModal({ humanInvites: [invite] });
    onCopyHumanInvite.mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: "사람 초대 1 링크 복사" }));
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByRole("button", { name: "사람 초대 1 링크 복사됨" })).toBeTruthy();
    // No success box appears for the routine copy.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(/복사됐|만들었어요|공개됐어요|닫았어요/)).toBeNull();
    act(() => { vi.advanceTimersByTime(2000); });
    expect(screen.getByRole("button", { name: "사람 초대 1 링크 복사" })).toBeTruthy();
    expect(onCopyHumanInvite).toHaveBeenCalledWith("current");
  } finally {
    vi.useRealTimers();
  }
});

it("renders an error status without any success styling", () => {
  renderInviteModal({ copyStatus: "보안 초대 링크 복사 실패" });
  const alert = screen.getByRole("alert");
  expect(alert.textContent).toBe("보안 초대 링크 복사 실패");
  expect(alert.className).not.toContain("success");
});

it("shows safe external-access failure copy", () => {
  renderInviteModal({ publicAccess: false, phase: "error", lastError: "cloudflared closed its output before exiting" });
  expect(screen.getByText("외부 접속을 열지 못했어요. 잠시 후 다시 시도해 주세요.")).toBeTruthy();
  expect(screen.queryByText(/cloudflared/)).toBeNull();
});

it("shows the daily quota line instead of an open claim while publish is parked", () => {
  renderInviteModal({ quotaParkedUntil: Math.floor(Date.now() / 1000) + 3600 });
  expect(screen.queryByText("외부 접속 열림")).toBeNull();
  expect(screen.getByText(/오늘 주소 등록 한도를 다 써서 친구들은 .* 이후에 들어올 수 있어요\./)).toBeTruthy();
});

it("keeps the open claim once the quota park has expired", () => {
  renderInviteModal({ quotaParkedUntil: Math.floor(Date.now() / 1000) - 10 });
  expect(screen.getByText("외부 접속 열림")).toBeTruthy();
  expect(screen.queryByText(/주소 등록 한도/)).toBeNull();
});

it.each([0, 1, 3, 370])("formats expiry %s local calendar days away without raw ISO", (days) => {
  vi.useFakeTimers();
  try {
    const now = new Date(2026, 9, 5, 23, 30);
    vi.setSystemTime(now);
    const expiry = new Date(2026, 9, 5 + days, 20, 40);
    const iso = expiry.toISOString();
    renderInviteModal({ humanInvites: [{ key: "expiry", displayName: "Guest", maxUses: 1, ttlSeconds: 86400,
      expiresAt: iso, expired: false, retired: false, originCurrent: true, authorityCurrent: true,
      revocation: "idle", copyUrl: "https://host.example/join?token=fixture" }] });
    const day = days === 0 ? "오늘" : days === 1 ? "내일"
      : expiry.toLocaleDateString("ko-KR", { ...(days === 370 ? { year: "numeric" as const } : {}), month: "long", day: "numeric" });
    expect(screen.getByText(new RegExp(`${day} 오후 8:40 만료`))).toBeTruthy();
    expect(document.body.textContent).not.toContain(iso);
  } finally { vi.useRealTimers(); }
});
