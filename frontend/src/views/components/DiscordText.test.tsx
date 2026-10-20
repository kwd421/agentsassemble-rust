import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import DiscordText, { KnownChannels, plainMentionText, UNKNOWN_MENTION_LABEL } from "./DiscordText";

describe("DiscordText", () => {
  it("colours only channels the room actually has", () => {
    const { container } = render(
      <KnownChannels names={["general", "Design Notes"]}>
        <DiscordText text="이전 턴(#107)에서 말한 내용은 #general 과 #design notes 채널에 있어요. #없는채널 은 아니에요." />
      </KnownChannels>
    );
    const channels = [...container.querySelectorAll(".dc-channel-mention")].map((node) => node.textContent);
    expect(channels).toEqual(["#general"]);
    expect(container.textContent).toContain("이전 턴(#107)에서");
    expect(container.textContent).toContain("#없는채널");
  });

  it("preserves structured provider replies as GitHub-flavored markdown", () => {
    const { container } = render(
      <DiscordText
        text={[
          "First paragraph.",
          "",
          "Second paragraph.",
          "",
          "| Item | Status | Note |",
          "| --- | --- | --- |",
          "| Table | OK | Three columns |",
          "",
          "- first item",
          "- second item",
          "",
          "Inline `ok`.",
        ].join("\n")}
      />
    );

    expect(container.querySelectorAll("p")).toHaveLength(3);
    expect(container.querySelector("table")).not.toBeNull();
    expect(screen.getByRole("columnheader", { name: "Item" })).toBeTruthy();
    expect(screen.getByRole("cell", { name: "Three columns" })).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(container.querySelector("code")?.textContent).toBe("ok");
  });

  it("preserves room mentions while blocking raw html", () => {
    const { container } = render(<DiscordText text={"@codex **확인** <script>alert(1)</script>"} />);

    expect(container.querySelector(".dc-mention")?.textContent).toBe("@codex");
    expect(container.querySelector("strong")?.textContent).toBe("확인");
    expect(container.querySelector("script")).toBeNull();
  });

  it("renders a durable participant mention with the current display name", () => {
    const { container, rerender } = render(
      <DiscordText
        text={"<@agent-uid-123> 확인"}
        mentionLabels={{ "agent-uid-123": "Opus 5" }}
      />
    );

    expect(container.querySelector(".dc-mention")?.textContent).toBe("@Opus 5");

    rerender(
      <DiscordText
        text={"<@agent-uid-123> 확인"}
        mentionLabels={{ "agent-uid-123": "Opus Prime" }}
      />
    );
    expect(container.querySelector(".dc-mention")?.textContent).toBe("@Opus Prime");
  });

  it("renders a left member's stored mention as a neutral label, never the raw id", () => {
    const departedId = "member-2412bc67-e8fe-44e8-8fbc-639004ba27af";
    const { container } = render(
      <DiscordText
        text={`<@${departedId}> hi there`}
        mentionLabels={{ "agent-uid-123": "Opus 5" }}
      />
    );

    const pill = container.querySelector(".dc-mention");
    expect(pill?.textContent).toBe(`@${UNKNOWN_MENTION_LABEL}`);
    expect(container.textContent).not.toContain(departedId);
  });

  it("keeps numeric ranges with single tildes while rendering explicit strikethrough", () => {
    const { container } = render(
      <DiscordText text={"game.py:385~393, routes.py:63~67·84~88, ~~removed~~"} />
    );

    expect(container.textContent).toContain("game.py:385~393");
    expect(container.textContent).toContain("routes.py:63~67·84~88");
    expect(container.querySelectorAll("del")).toHaveLength(1);
    expect(container.querySelector("del")?.textContent).toBe("removed");
  });

  it("does not fetch a remote markdown image until the viewer opens its inert link", () => {
    const { container } = render(
      <DiscordText text={"![tracking pixel](https://tracker.invalid/pixel.png)"} />
    );

    expect(container.querySelector("img")).toBeNull();
    expect(
      screen.getByRole("link", { name: "tracking pixel" }).getAttribute("href")
    ).toBe("https://tracker.invalid/pixel.png");
  });

  it("reads stored mentions as names in one-line previews", () => {
    expect(plainMentionText("<@connector-7c0c> 안녕! <@gone> 잘 가", { "connector-7c0c": "안티그래비티" }))
      .toBe(`@안티그래비티 안녕! @${UNKNOWN_MENTION_LABEL} 잘 가`);
  });
});
