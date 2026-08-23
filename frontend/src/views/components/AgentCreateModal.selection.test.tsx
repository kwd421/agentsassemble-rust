import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { claudeProvider, codexProvider } from "./AgentCreateModal.testProviders";

afterEach(cleanup);

it("requires an explicit provider selection when the modal opens", () => {
  render(
    <AgentCreateModal
      open
      meetingId="room-a"
      roomLabel="Room A"
      catalogRevision="cat-explicit"
      providers={[codexProvider(), claudeProvider()]}
      onClose={() => undefined}
      onCreate={vi.fn()}
    />
  );

  expect(screen.getByText("사용할 provider를 선택하세요.")).toBeTruthy();
  expect(screen.queryByRole("combobox", { name: "모델" })).toBeNull();
  expect(screen.getByRole("listitem", { name: "Codex" }).getAttribute("data-active")).toBe(
    "false"
  );
  expect(
    screen.getByRole("listitem", { name: "Claude Code" }).getAttribute("data-active")
  ).toBe("false");
  expect(
    screen.getByRole("listitem", { name: "Codex" }).querySelector('[data-provider-brand="codex"]')
  ).not.toBeNull();
  expect(
    screen
      .getByRole("listitem", { name: "Claude Code" })
      .querySelector('[data-provider-brand="claude"]')
  ).not.toBeNull();
  expect(screen.getByRole("button", { name: "추가" }).hasAttribute("disabled")).toBe(true);
});
