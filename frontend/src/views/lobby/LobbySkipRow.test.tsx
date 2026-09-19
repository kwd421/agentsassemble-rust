import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { LobbyEvent } from "../../api";
import { LobbySkipRow } from "./LobbyEventRows";

afterEach(cleanup);

const event: LobbyEvent = {
  id: "turn-declined:a", kind: "system", name: "Sonnet", side: "other", created_at: "",
  message: "차례 넘김 — Sonnet(나를 부른 게 아님), DeepSeek(덧붙일 말 없음)",
  skips: [
    { participant_id: "claude", name: "Sonnet", reason: "나를 부른 게 아님" },
    { participant_id: "deepseek", name: "DeepSeek", reason: "덧붙일 말 없음" },
  ],
};

describe("LobbySkipRow", () => {
  it("stays a quiet line until pointed at, then names every skipped agent", () => {
    const { container } = render(<LobbySkipRow event={event} />);
    expect(screen.getByText("차례 넘김 2")).toBeTruthy();
    expect(screen.queryByText("Sonnet")).toBeNull();

    const row = container.querySelector(".dc-system-divider")!;
    fireEvent.mouseEnter(row);
    expect(screen.getByText("Sonnet")).toBeTruthy();
    expect(screen.getByText("DeepSeek")).toBeTruthy();
    expect(screen.getByText("덧붙일 말 없음")).toBeTruthy();

    fireEvent.mouseLeave(row);
    expect(screen.queryByText("Sonnet")).toBeNull();
  });

  it("keeps the whole summary available without hovering", () => {
    render(<LobbySkipRow event={event} />);
    expect(screen.getByLabelText(event.message)).toBeTruthy();
  });
});
