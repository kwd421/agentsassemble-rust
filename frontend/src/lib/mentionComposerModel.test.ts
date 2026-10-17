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
  it("binds a picked mention to the participant id so send needs no label lookup", () => {
    const query = mentionQueryAtCursor("@cl", 3);
    const next = insertMentionText("@cl", 3, query, agent);
    expect(next.message).toBe("<@opencode-87a106c9-test> ");
  });

  it("keeps the picked wire markup untouched at send time", () => {
    const query = mentionQueryAtCursor("@cl", 3);
    const inserted = insertMentionText("확인해 줘 @cl", 9, query && { ...query, start: 6 }, agent);
    expect(inserted.message).toBe("확인해 줘 <@opencode-87a106c9-test> ");
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

  it("binds a picked duplicate name to the selected participant, never the first match", () => {
    const first: Mentionable = { token: "agent-first", label: "동일" };
    const second: Mentionable = { token: "agent-second", label: "동일" };
    const query = mentionQueryAtCursor("@동", 2);
    const inserted = insertMentionText("@동", 2, query, second);
    expect(resolveMentionMarkup(inserted.message, [first, second])).toBe(
      "<@agent-second> "
    );
  });

  it("leaves an ambiguous typed label as plain text instead of notifying someone", () => {
    const first: Mentionable = { token: "agent-first", label: "동일" };
    const second: Mentionable = { token: "agent-second", label: "동일" };
    expect(resolveMentionMarkup("안녕 @동일", [first, second])).toBe(
      "안녕 @동일"
    );
  });
});
