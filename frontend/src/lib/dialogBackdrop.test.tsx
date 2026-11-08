import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { backdropDismiss } from "./dialogBackdrop";

it("closes on a backdrop click but not on a drag that started inside and ended on the backdrop", () => {
  const onDismiss = vi.fn();
  render(<div role="dialog" aria-label="새 방 만들기" {...backdropDismiss(onDismiss)}><input aria-label="방 이름" /></div>);
  const dialog = screen.getByRole("dialog");
  fireEvent.pointerDown(screen.getByLabelText("방 이름"));
  fireEvent.click(dialog);
  expect(onDismiss).not.toHaveBeenCalled();
  fireEvent.click(dialog);
  expect(onDismiss).not.toHaveBeenCalled();
  fireEvent.pointerDown(dialog);
  fireEvent.click(dialog);
  expect(onDismiss).toHaveBeenCalledOnce();
});
