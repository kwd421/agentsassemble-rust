import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { useState } from "react";
import { useCompanionInvites } from "../../app/useCompanionInvites";
import { createCompanionAttendeeInvite } from "../../api/attendeeInvite";
import type { RoomGuestSession } from "../../lib/roomGuestSession";
import type { RoomEvent } from "../../api";
import { codexProvider } from "./AgentCreateModal.testProviders";
import OwnComputerCreateModal from "./OwnComputerCreateModal";
vi.mock("../../api/attendeeInvite", async (original) => ({ ...await original<typeof import("../../api/attendeeInvite")>(), createCompanionAttendeeInvite: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: vi.fn(() => false) }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it.each([60_000, 600_000])("preserves the draft and admission with %i ms left, showing only urgent expiry", async (remaining) => {
  const owner = { roomUid: "uid", meetingId: "general", sessionToken: "human-private", expiresAt: "2099-01-01T00:00:00Z" } as RoomGuestSession;
  const expiry = Date.now() + remaining;
  const issued = { origin: window.location.origin, expiresAtMs: expiry, result: {
    request_id: "request", room_id: "general", room_uid: "uid", invite_id: "invite", display_name: "My own AI", provider: "codex",
    attend_command: "assemble room attend --provider codex", join_url: `${window.location.origin}/join?token=fixture`, expires_at: new Date(expiry).toISOString(),
  } };
  vi.mocked(createCompanionAttendeeInvite).mockResolvedValue(issued);
  const onHost = vi.fn();
  function Surface({ ready, events = [] }: { ready: boolean; events?: RoomEvent[] }) {
    const controls = useCompanionInvites(owner, events);
    return <OwnComputerCreateModal roomLabel="Remote room" providers={[{ ...codexProvider(), startable: ready, default_model: "HOST_ONLY_MODEL" }]}
      controls={controls} onClose={() => {}} locationChoice={<button onClick={onHost}>서버 컴퓨터</button>} />;
  }
  const view = render(<Surface ready={false} />);
  fireEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("Codex");
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "My own AI" } });
  expect(screen.queryByRole("combobox", { name: "모델" })).toBeNull();
  expect(screen.queryByText("HOST_ONLY_MODEL")).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "이 컴퓨터에서 계속" })));
  expect(screen.getByRole("link", { name: "이 컴퓨터에서 설정하기" }).getAttribute("href")).toMatch(/^agentsassemble:\/\/attend#packet=/);
  expect(screen.queryByText(/ · codex/)).toBeNull();
  expect(Boolean(screen.queryByText(/초대가 곧 만료돼요/))).toBe(remaining < 120_000);
  view.rerender(<Surface ready />);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("My own AI");
  expect(screen.queryByText(/방 참가가 확인/)).toBeNull();
  view.rerender(<Surface ready events={[{ v: 1, id: "event", seq: 1, created_at: "2026-09-10T00:00:00Z", actor: { participant_id: "server", participant_type: "system" }, type: "agent_session_created", room_id: "general", attendee_invite_id: "invite" } as RoomEvent]} />);
  expect(screen.getByText(/방 참가가 확인/)).toBeTruthy();
  expect(screen.getByRole("link", { name: "실행 상태 열기" })).toBeTruthy();
  expect(createCompanionAttendeeInvite).toHaveBeenCalledTimes(1);
  expect(vi.mocked(createCompanionAttendeeInvite).mock.calls[0][1]).toEqual({ request_id: expect.any(String), provider: "codex", display_name: "My own AI" });
  fireEvent.click(screen.getByRole("button", { name: "서버 컴퓨터" }));
  expect(onHost).toHaveBeenCalledTimes(1);
});


it("closes desktop creation after handoff, leaves status and never retains setup links", async () => {
  vi.mocked(isDesktopWebview).mockReturnValue(true);
  const owner = { roomUid: "uid", meetingId: "general", sessionToken: "human-private", expiresAt: "2099-01-01T00:00:00Z" } as RoomGuestSession;
  vi.mocked(createCompanionAttendeeInvite).mockResolvedValue({ origin: window.location.origin, expiresAtMs: Date.now() + 600000, result: {
    request_id: "request", room_id: "general", room_uid: "uid", invite_id: "invite", display_name: "Codex", provider: "codex",
    attend_command: "assemble room attend --provider codex", join_url: `${window.location.origin}/join?token=fixture`, expires_at: "2099-01-01T00:00:00Z",
  } });
  function Surface() {
    const controls = useCompanionInvites(owner);
    const [open, setOpen] = useState(true);
    return <>{open ? <OwnComputerCreateModal roomLabel="Remote room" providers={[codexProvider()]} controls={controls} onClose={() => setOpen(false)} />
      : <p role="status">{controls.status}</p>}<button onClick={() => setOpen(true)}>다시 열기</button></>;
  }
  render(<Surface />);
  fireEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "이 컴퓨터에서 계속" })));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByRole("status").textContent).toBe("이 컴퓨터에 뜬 창에서 마저 설정해 주세요.");
  fireEvent.click(screen.getByRole("button", { name: "다시 열기" }));
  expect(screen.queryByRole("link")).toBeNull();
});
