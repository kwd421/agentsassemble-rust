import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import MentionInput from "./MentionInput";

describe("MentionInput", () => {
  afterEach(() => cleanup());

  it("shows the participant avatar and owner beside a mention candidate", () => {
    render(
      <MentionInput
        value="@"
        onChange={vi.fn()}
        ariaLabel="채팅 입력"
        mentionables={[
          {
            token: "agent-uid-123",
            label: "Opus 5",
            avatarImage: "/api/avatars/opus.png",
            detail: "SeiNel의 에이전트",
            participantKind: "agent",
            providerKind: "claude_code",
          },
        ]}
      />
    );

    fireEvent.select(screen.getByLabelText("채팅 입력"), {
      target: { selectionStart: 1 },
    });

    const candidate = screen.getByRole("option", {
      name: "Opus 5, SeiNel의 에이전트",
    });
    expect(candidate.querySelector("img")?.getAttribute("src")).toBe(
      "/api/avatars/opus.png"
    );
  });
});

it("shows a picked mention as its name and deletes it as one piece", () => {
  const onChange = vi.fn();
  render(<MentionInput ariaLabel="입력" value="hi <@agent-1> there" onChange={onChange}
    mentionables={[{ token: "agent-1", label: "리뷰-AI", participantKind: "agent" }]} />);
  const input = screen.getByLabelText("입력") as HTMLTextAreaElement;
  expect(input.value).toBe("hi @리뷰-AI there");
  fireEvent.change(input, { target: { value: "hi @리뷰-A there" } });
  expect(onChange).toHaveBeenLastCalledWith("hi  there");
  fireEvent.change(input, { target: { value: "hi @리뷰-AI there!" } });
  expect(onChange).toHaveBeenLastCalledWith("hi <@agent-1> there!");
});
