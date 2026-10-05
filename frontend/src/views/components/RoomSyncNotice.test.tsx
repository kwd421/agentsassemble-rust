import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import RoomSyncNotice from "./RoomSyncNotice";

afterEach(cleanup);

describe("RoomSyncNotice", () => {
  it("keeps a detected room-state mismatch visible while recovery is in progress", () => {
    render(
      <RoomSyncNotice
        issue={{
          category: "event_sequence_gap",
          message: "Room event sequence gap detected.",
        }}
      />
    );

    expect(screen.getByRole("status").textContent).toContain(
      "서버 원본으로 다시 동기화"
    );
  });

  it("does not show a recovery notice during normal synchronization", () => {
    render(<RoomSyncNotice issue={null} />);

    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows a failed room connection instead of leaving the room loading silently", () => {
    render(
      <RoomSyncNotice
        issue={{
          category: "socket_connection_failed",
          message: "Room WebSocket connection failed.",
        }}
      />
    );

    expect(screen.getByRole("status").textContent).toContain(
      "방 서버에 연결하지 못했어요"
    );
  });

  it("labels cached room-directory state as unconfirmed", () => {
    render(
      <RoomSyncNotice
        issue={{
          category: "room_directory_unconfirmed",
          message: "pending",
        }}
      />
    );

    expect(screen.getByRole("status").textContent).toContain(
      "서버 원본과 확인"
    );
  });
  it("keeps the original lifecycle target retryable without a room and shows only confirmed completion", () => {
    const lifecycle = { pending: { serverId: "server", authorityLineageId: "lineage", requestId: "same",
      roomId: "removed", roomUid: "exact", action: "room.delete" as const, confirmationName: "Previous room" },
      busy: false, error: "", notice: "방 삭제를 처리하고 있어요.", retry: vi.fn(), dismissNotice: vi.fn() };
    const { rerender } = render(<RoomSyncNotice issue={null} lifecycle={lifecycle} />);
    expect(screen.getByRole("status").textContent).toContain("Previous room");
    fireEvent.click(screen.getByRole("button", { name: "완료 여부 확인" }));
    expect(lifecycle.retry).toHaveBeenCalledOnce();
    rerender(<RoomSyncNotice issue={null} lifecycle={{ ...lifecycle, pending: null, notice: "방이 삭제됐어요." }} />);
    expect(screen.queryByRole("button", { name: "완료 여부 확인" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "닫기" }));
    expect(lifecycle.dismissNotice).toHaveBeenCalledOnce();
  });

});

it("keeps room-management feedback usable alongside a single outage banner", () => {
  const dismissNotice = vi.fn();
  const { container } = render(<RoomSyncNotice connectionMessage="로그인 서버 연결이 끊겼어요."
    issue={{ category: "room_directory_unavailable", message: "offline" }}
    lifecycle={{ pending: null, busy: false, notice: "방 관리 결과", error: "", retry: vi.fn(), dismissNotice }} />);
  expect(container.querySelectorAll(".dc-connection-banner")).toHaveLength(1);
  expect(screen.getByText("방 관리 결과")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "닫기" }));
  expect(dismissNotice).toHaveBeenCalledOnce();
});
