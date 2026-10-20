import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import ImageCropper from "./ImageCropper";

// A deferred toBlob: the gap between 적용 and the finished image is where a second
// click, a cancel or an unmount can land.
let pendingBlobs: ((blob: Blob | null) => void)[] = [];

beforeEach(() => {
  pendingBlobs = [];
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: () => "blob:crop-source" });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: () => undefined });
  vi.stubGlobal("Image", class {
    naturalWidth = 400;
    naturalHeight = 400;
    private listeners: Record<string, () => void> = {};
    addEventListener(type: string, listener: () => void) { this.listeners[type] = listener; }
    set src(_value: string) { queueMicrotask(() => this.listeners.load?.()); }
  });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect() {}, save() {}, beginPath() {}, arc() {}, clip() {}, drawImage() {}, restore() {},
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation((callback) => { pendingBlobs.push(callback); });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

async function ready(onCropped = vi.fn(), onCancel = vi.fn()) {
  const view = render(<ImageCropper file={new File(["x"], "a.png", { type: "image/png" })} onCropped={onCropped} onCancel={onCancel} />);
  const apply = screen.getByRole("button", { name: "적용" }) as HTMLButtonElement;
  await waitFor(() => expect(apply.disabled).toBe(false));
  return { ...view, apply, onCropped, onCancel };
}

it("prepares one image for repeated 적용 clicks", async () => {
  const { apply, onCropped } = await ready();
  fireEvent.click(apply);
  fireEvent.click(apply);
  await waitFor(() => expect(pendingBlobs).toHaveLength(1));
  act(() => pendingBlobs[0](new Blob(["png"], { type: "image/png" })));
  expect(onCropped).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(apply.disabled).toBe(false));
});

it("delivers nothing after 취소 or unmount while the image is being prepared", async () => {
  const first = await ready();
  fireEvent.click(first.apply);
  await waitFor(() => expect(pendingBlobs).toHaveLength(1));
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  act(() => pendingBlobs[0](new Blob(["png"], { type: "image/png" })));
  expect(first.onCancel).toHaveBeenCalledTimes(1);
  expect(first.onCropped).not.toHaveBeenCalled();
  first.unmount();

  const second = await ready();
  fireEvent.click(second.apply);
  await waitFor(() => expect(pendingBlobs).toHaveLength(2));
  second.unmount();
  pendingBlobs[1](new Blob(["png"], { type: "image/png" }));
  expect(second.onCropped).not.toHaveBeenCalled();
});
