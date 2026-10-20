import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../../test/agentSession";
import MemberDetailModal from "./MemberDetailModal";
import type { MemberEntry } from "./memberTypes";

afterEach(cleanup);

it("discards a name being typed when Escape closes the card", () => {
  const session = agentSessionFixture({ display_name: "Gemini", enabled: false, runtime_status: "stopped" });
  const onSave = vi.fn();
  const onClose = vi.fn();
  const entry = { id: "a", agentSession: session, displayName: "Gemini", detail: "", role: "member", owner: true } as unknown as MemberEntry;
  render(<MemberDetailModal entry={entry} onClose={onClose} onAgentProfileUpdate={onSave} />);
  const dialog = document.querySelector("dialog") as HTMLDialogElement;
  expect(document.activeElement).toBe(dialog);
  const name = screen.getByLabelText("표시 이름") as HTMLInputElement;
  name.focus();
  fireEvent.change(name, { target: { value: "Gemini 테스트" } });
  // An Escape eaten by IME composition reaches only the dialog's cancel.
  fireEvent(dialog, new Event("cancel", { cancelable: true }));
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(onSave).not.toHaveBeenCalled();
  expect(name.value).toBe("Gemini");
});
