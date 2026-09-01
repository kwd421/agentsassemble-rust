import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HumanInvitePresentation } from "../../app/useManagedHumanInvites";
import RoomInviteModal from "./RoomInviteModal";

afterEach(cleanup);

function renderInviteModal({
  publicAccess = true,
  activeWithoutUrl = false,
  phase,
  requestState = "idle",
  humanInvites = [],
}: {
  publicAccess?: boolean;
  activeWithoutUrl?: boolean;
  phase?: "stopped" | "starting" | "running" | "stopping" | "error";
  requestState?: "idle" | "starting" | "stopping";
  humanInvites?: HumanInvitePresentation[];
} = {}) {
  const onGenerateSecureInvite = vi.fn();
  const onGenerateAgentInvite = vi.fn();
  const onStopTunnel = vi.fn();
  const onCopyHumanInvite = vi.fn();
  const onRevokeHumanInvite = vi.fn();
  const tunnelPhase = phase || (publicAccess || activeWithoutUrl ? "running" : "stopped");
  const tunnelActive = ["starting", "running", "stopping"].includes(tunnelPhase);
  render(
    <RoomInviteModal
      roomLabel="제품 방"
      humanInvites={humanInvites}
      agentInviteUrl=""
      operatorPairingUrl=""
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
      onGenerateAgentInvite={onGenerateAgentInvite}
      onCopyAgentInvite={vi.fn()}
      onGenerateOperatorPairing={vi.fn()}
      onCopyOperatorPairing={vi.fn()}
      onStartTunnel={vi.fn()}
      onStopTunnel={onStopTunnel}
    />
  );
  return {
    onGenerateSecureInvite,
    onGenerateAgentInvite,
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

  it("requires an explicit public-access confirmation for an external AI session", () => {
    const { onGenerateAgentInvite } = renderInviteModal({ publicAccess: false });

    expect(screen.getByRole("heading", { name: "외부 AI 세션 초대" })).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "외부 AI 세션 초대 링크 생성" })
    );

    expect(onGenerateAgentInvite).not.toHaveBeenCalled();
    expect(screen.getByRole("alertdialog", { name: "외부 접속을 열까요?" })).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "외부 접속 열고 링크 만들기" })
    );

    expect(onGenerateAgentInvite).toHaveBeenCalledWith(true);
  });
});
