import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../../test/agentSession";
import AgentIdentitySettings from "./AgentIdentitySettings";

afterEach(cleanup);

it("saves only identity through the server callback and exposes failed saves", async () => {
  const session = agentSessionFixture({ display_name: "Original", runtime_status: "busy" });
  const onSave = vi.fn().mockRejectedValueOnce(new Error("Save failed")).mockResolvedValue(undefined);
  const { rerender } = render(<AgentIdentitySettings session={session} onSave={onSave} />);
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "New name" } });
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Save failed"));
  expect(onSave).toHaveBeenCalledWith(session, { display_name: "New name" });
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("New name");
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("에이전트 프로필 저장됨"));
  rerender(<AgentIdentitySettings session={{ ...session, display_name: "From server" }} onSave={onSave} />);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("From server");
});
