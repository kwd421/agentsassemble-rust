import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import RoomSettingTextInput from "./RoomSettingTextInput";

afterEach(cleanup);

it("retains typing across canonical echoes and commits once when focus leaves", () => {
  const onCommit = vi.fn();
  const normalize = (value: string) => value;
  const view = render(<RoomSettingTextInput value="Room" normalize={normalize} onCommit={onCommit} />);
  const input = screen.getByRole("textbox") as HTMLInputElement;
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value: "Phase4 Room" } });
  view.rerender(<RoomSettingTextInput value="Other canonical value" normalize={normalize} onCommit={onCommit} />);
  expect(input.value).toBe("Phase4 Room");
  expect(onCommit).not.toHaveBeenCalled();
  fireEvent.blur(input);
  expect(onCommit).toHaveBeenCalledExactlyOnceWith("Phase4 Room");
  view.rerender(<RoomSettingTextInput value="Phase4 Room" normalize={normalize} onCommit={onCommit} />);
  expect(input.value).toBe("Phase4 Room");
});
