import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { MessageReply, replyMentions } from "./MessageReply";

afterEach(cleanup);

it("marks the original author with @ only when the reply calls them, as Discord does", () => {
  expect(replyMentions("<@operator-local> 확인했어요", "operator-local")).toBe(true);
  expect(replyMentions("확인했어요", "operator-local")).toBe(false);
  const { rerender } = render(<MessageReply source={{ eventId: "e", author: "테스터", text: "원문", mentioned: true }} />);
  expect(screen.getByText("@테스터")).toBeTruthy();
  rerender(<MessageReply source={{ eventId: "e", author: "테스터", text: "원문" }} />);
  expect(screen.getByText("테스터")).toBeTruthy();
  expect(screen.queryByText("@테스터")).toBeNull();
});
