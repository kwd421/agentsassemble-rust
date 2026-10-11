import { useContext, useEffect } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AccountDeletionSurface from "./AccountDeletionSurface";
import { AccountDeletionContext } from "./AccountDeletionContext";
const mocks = vi.hoisted(() => ({ close: vi.fn(), desktop: false, dispose: vi.fn() }));
vi.mock("../../lib/remote/remoteWorkspace", () => ({ closeRemoteWorkspace: mocks.close }));
vi.mock("../../lib/central/identity", () => ({ centralAccountEntryUrl: () => "https://central.example/" }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => mocks.desktop }));
vi.mock("./AccountDeletionSettings", () => ({ AccountDeletionFlow: ({ onDeleted, onClose }: { onDeleted: () => void; onClose: () => void }) => <>
  <button onClick={onDeleted}>Complete deletion</button><button onClick={onClose}>확인</button><button onClick={onClose}>계정 탈퇴 닫기</button>
</> }));
function Room() {
  const open = useContext(AccountDeletionContext);
  useEffect(() => mocks.dispose, []);
  return <button onClick={() => open?.()}>Old room identity and reconnect banner</button>;
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.clearAllMocks(); });
it.each([[false, "확인"], [false, "계정 탈퇴 닫기"], [true, "확인"], [true, "계정 탈퇴 닫기"]] as const)("tears down the room on completion and returns to signed-out startup (desktop=%s, %s)", (desktop, control) => {
  mocks.desktop = desktop;
  const replace = vi.fn(), actualWindow = window;
  vi.stubGlobal("window", new Proxy(actualWindow, { get: (target, key) => key === "location" ? { replace } : Reflect.get(target, key) }));
  vi.spyOn(HTMLDialogElement.prototype, "showModal").mockImplementation(function (this: HTMLDialogElement) { this.open = true; });
  vi.spyOn(HTMLDialogElement.prototype, "close").mockImplementation(function (this: HTMLDialogElement) { this.open = false; });
  render(<AccountDeletionSurface><Room /></AccountDeletionSurface>);
  fireEvent.click(screen.getByText("Old room identity and reconnect banner"));
  fireEvent.click(screen.getByText("Complete deletion"));
  expect(screen.queryByText("Old room identity and reconnect banner")).toBeNull();
  expect(mocks.dispose).toHaveBeenCalledOnce();
  expect(mocks.close).toHaveBeenCalledOnce();
  expect(replace).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText(control));
  expect(replace).toHaveBeenCalledExactlyOnceWith(desktop ? "/" : "https://central.example/");
});
