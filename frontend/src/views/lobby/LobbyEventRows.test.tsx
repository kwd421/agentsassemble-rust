import { describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { LobbySystemRow } from "./LobbyEventRows";
import type { LobbyEvent } from "../../api";

describe("LobbySystemRow", () => {
  it("renders delegation sentences as plain text so display names cannot inject links or mentions", () => {
    cleanup();
    const event = {
      id: "e1",
      kind: "system",
      message: "actor created a text channel on behalf of owner.",
      name: "actor",
      delegation: {
        action: "create_text_channel",
        targetId: "ch-1",
        onBehalfOfParticipantId: "owner-1",
        actorParticipantId: "actor-1",
      },
    } as unknown as LobbyEvent;
    render(
      <LobbySystemRow
        event={event}
        mentionLabels={{ "관리자": "someone-else" }}
        nameSources={{
          participantProfiles: {
            "actor-1": { displayName: "[클릭](https://evil.example) @관리자" },
            "owner-1": { displayName: "주인" },
          },
          channelLabels: { "ch-1": "공지" },
        }}
      />
    );
    const status = screen.getByRole("status");
    expect(status.textContent).toContain("[클릭](https://evil.example) @관리자");
    expect(status.querySelector("a")).toBeNull();
    expect(status.querySelector(".dc-mention")).toBeNull();
  });
});
