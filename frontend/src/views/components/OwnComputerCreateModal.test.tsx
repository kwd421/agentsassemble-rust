import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useCompanionInvites } from "../../app/useCompanionInvites";
import { createCompanionAttendeeInvite } from "../../api/attendeeInvite";
import type { RoomGuestSession } from "../../lib/roomGuestSession";
import type { RoomEvent } from "../../api";
import { codexProvider } from "./AgentCreateModal.testProviders";
import OwnComputerCreateModal from "./OwnComputerCreateModal";
vi.mock("../../api/attendeeInvite", async (original) => ({ ...await original<typeof import("../../api/attendeeInvite")>(), createCompanionAttendeeInvite: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("preserves browser inputs across cold/ready host catalogs and waits for exact admission while retaining local status access", async () => {
  const owner = { roomUid: "uid", meetingId: "general", sessionToken: "human-private", expiresAt: "2099-01-01T00:00:00Z" } as RoomGuestSession;
  const issued = { origin: window.location.origin, expiresAtMs: Date.now() + 60_000, result: {
    request_id: "request", room_id: "general", room_uid: "uid", invite_id: "invite", display_name: "My own AI", provider: "codex",
    attend_command: "assemble room attend --provider codex", join_url: `${window.location.origin}/join?token=fixture`, expires_at: "2099-01-01T00:00:00Z",
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
