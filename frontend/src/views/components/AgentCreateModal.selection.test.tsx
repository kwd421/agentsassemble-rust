import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { codexProvider } from "./AgentCreateModal.testProviders";

afterEach(cleanup);

describe("AgentCreateModal provider selection", () => {
  it("does not ask for a display name before a provider is selected", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        providers={[codexProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.queryByLabelText("표시 이름")).toBeNull();
    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
    expect(screen.getByLabelText("표시 이름")).toBeTruthy();
  });
});
