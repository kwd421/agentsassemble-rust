import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import RoomInviteModal from "./RoomInviteModal";

afterEach(cleanup);

function renderInviteModal({
  publicAccess = true,
  activeWithoutUrl = false,
  phase,
  requestState = "idle",
}: {
  publicAccess?: boolean;
  activeWithoutUrl?: boolean;
  phase?: "stopped" | "starting" | "running" | "stopping" | "error";
  requestState?: "idle" | "starting" | "stopping";
} = {}) {
  const onGenerateSecureInvite = vi.fn();
  const onGenerateAgentInvite = vi.fn();
  const onStopTunnel = vi.fn();
  const tunnelPhase = phase || (publicAccess || activeWithoutUrl ? "running" : "stopped");
  const tunnelActive = ["starting", "running", "stopping"].includes(tunnelPhase);
  render(
    <RoomInviteModal
      roomLabel="제품 방"
      secureInviteUrl=""
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
      friends={[]}
      onClose={vi.fn()}
      onGenerateSecureInvite={onGenerateSecureInvite}
      onCopy={vi.fn()}
      onGenerateAgentInvite={onGenerateAgentInvite}
      onCopyAgentInvite={vi.fn()}
      onGenerateOperatorPairing={vi.fn()}
      onCopyOperatorPairing={vi.fn()}
      onStartTunnel={vi.fn()}
      onStopTunnel={onStopTunnel}
      onInviteFriend={vi.fn()}
    />
  );
  return { onGenerateSecureInvite, onGenerateAgentInvite, onStopTunnel };
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
