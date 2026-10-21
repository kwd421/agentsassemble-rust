import { describe, expect, it } from "vitest";
import { applyDisplayEdit, draftDisplay, mentionLabels, replaceDisplayRange } from "./mentionDraftDisplay";

const labels = mentionLabels([{ token: "agent-1", label: "리뷰-AI" }, { token: "human-2", label: "Bob" }]);

describe("mention draft display", () => {
  it("shows picked mentions as names and keeps unknown ids as written", () => {
    expect(draftDisplay("<@agent-1> 봐 줘 <@ghost>", labels).text).toBe("@리뷰-AI 봐 줘 <@ghost>");
  });

  it("maps typing around a mention back onto the stored text", () => {
    const draft = "hi <@agent-1> there";
    expect(applyDisplayEdit(draft, labels, "hi @리뷰-AI there!").markup).toBe("hi <@agent-1> there!");
    expect(applyDisplayEdit(draft, labels, "oh hi @리뷰-AI there").markup).toBe("oh hi <@agent-1> there");
    expect(applyDisplayEdit(draft, labels, "hi @리뷰-AI, there").markup).toBe("hi <@agent-1>, there");
  });

  it("removes a mention as one piece when an edit touches it", () => {
    const draft = "hi <@agent-1> there";
    // Backspace at the end of the name.
    const removed = applyDisplayEdit(draft, labels, "hi @리뷰-A there");
    expect(removed).toEqual({ markup: "hi  there", cursor: 3 });
    // Typing inside the name keeps the typed text and drops the mention.
    expect(applyDisplayEdit(draft, labels, "hi @리뷰x-AI there").markup).toBe("hi x there");
  });

  it("inserts at a display range, after mentions, in stored positions", () => {
    const draft = "<@human-2> and <@agent-1>";
    const end = draftDisplay(draft, labels).text.length;
    expect(replaceDisplayRange(draft, labels, end, end, " 🙂")).toEqual({
      markup: "<@human-2> and <@agent-1> 🙂",
      cursor: end + 3,
    });
    expect(replaceDisplayRange(draft, labels, 5, 8, "or").markup).toBe("<@human-2> or <@agent-1>");
  });
});
