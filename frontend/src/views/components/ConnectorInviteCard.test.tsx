import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConnectorInviteCard } from "./ConnectorInviteCard";

afterEach(cleanup);

describe("ConnectorInviteCard reconnect", () => {
  it("offers a reconnect invitation per external AI seat and labels it by seat", () => {
    const reconnect = vi.fn();
    const controls = {
      invites: [{ key: "i1", expiresAt: new Date(Date.now() + 60_000).toISOString(), copyable: true, local: true, seatName: "아쎄이" }],
      creating: false, create: vi.fn(), reconnect, copy: vi.fn(),
    };
    render(<ConnectorInviteCard controls={controls} disabled={false} localOnly seats={[{ id: "connector-a", name: "아쎄이" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "아쎄이 다시 연결 초대 만들기" }));
    expect(reconnect).toHaveBeenCalledWith({ id: "connector-a", name: "아쎄이" });
    expect(screen.getByText("아쎄이 다시 연결 · 이 컴퓨터 전용")).toBeTruthy();
  });

  it("shows no reconnect section without seats", () => {
    const controls = { invites: [], creating: false, create: vi.fn(), reconnect: vi.fn(), copy: vi.fn() };
    render(<ConnectorInviteCard controls={controls} disabled={false} localOnly={false} />);
    expect(screen.queryByText("기존 외부 AI 다시 연결")).toBeNull();
  });
});
