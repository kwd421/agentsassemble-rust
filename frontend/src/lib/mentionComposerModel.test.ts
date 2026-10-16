import { describe, expect, it } from "vitest";
import {
  insertMentionText,
  mentionQueryAtCursor,
  resolveMentionMarkup,
  type Mentionable,
} from "./mentionComposerModel";

const agent: Mentionable = {
  token: "opencode-87a106c9-test",
  label: "claude-haiku-5-5",
  participantKind: "agent",
};

describe("mentionComposerModel", () => {
  it("inserts the readable name so the raw token never shows in the composer", () => {
    const query = mentionQueryAtCursor("@cl", 3);
    const next = insertMentionText("@cl", 3, query, agent);
    expect(next.message).toBe("@claude-haiku-5-5 ");
    expect(next.message).not.toContain("opencode-87a106c9-test");
    expect(next.message).not.toContain("<@");
  });

  it("resolves the readable name back to the wire markup at send time", () => {
    const query = mentionQueryAtCursor("@cl", 3);
    const inserted = insertMentionText("확인해 줘 @cl", 9, query && { ...query, start: 6 }, agent);
    expect(resolveMentionMarkup(inserted.message, [agent])).toBe(
      "확인해 줘 <@opencode-87a106c9-test> "
    );
  });

  it("leaves unknown handles, plain tokens and existing markup untouched", () => {
    expect(resolveMentionMarkup("안녕 @stranger", [agent])).toBe("안녕 @stranger");
    expect(resolveMentionMarkup("안녕 @opencode-87a106c9-test", [agent])).toBe(
      "안녕 @opencode-87a106c9-test"
    );
    expect(resolveMentionMarkup("안녕 <@opencode-87a106c9-test>", [agent])).toBe(
      "안녕 <@opencode-87a106c9-test>"
    );
    expect(resolveMentionMarkup("안녕 @claude-haiku-5-5", [])).toBe(
      "안녕 @claude-haiku-5-5"
    );
  });

  it("does not rewrite a mentionable whose label is the token itself", () => {
    const plain: Mentionable = { token: "lobby", label: "lobby" };
    expect(resolveMentionMarkup("가자 @lobby", [plain])).toBe("가자 @lobby");
  });

  it("prefers the longest display name when labels overlap", () => {
    const short: Mentionable = { token: "agent-short", label: "하쿠" };
    const long: Mentionable = { token: "agent-long", label: "하쿠 2호" };
    expect(resolveMentionMarkup("안녕 @하쿠 2호", [short, long])).toBe(
      "안녕 <@agent-long>"
    );
  });
});
