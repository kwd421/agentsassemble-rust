import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const transfer = vi.hoisted(() => ({ read: vi.fn() }));

vi.mock("../../api/messageAttachments", () => ({
  fetchMessageAttachmentBlob: transfer.read,
}));

import LobbyAttachments from "./LobbyAttachments";
import type { LobbyAttachmentRef } from "../../api/messageAttachments";

function attachment(hex = "a", image = true): LobbyAttachmentRef {
  const id = `ma_${hex.repeat(32)}`;
  return {
    id,
    filename: image ? `${hex}.png` : `${hex}.txt`,
    content_type: image ? "image/png" : "text/plain",
    size: 5,
    is_image: image,
    url: `/api/attachments/${id}?view=1`,
    download_url: `/api/attachments/${id}?download=1`,
  };
}

describe("LobbyAttachments", () => {
  const createObjectURL = vi.fn<(blob: Blob) => string>();
  const revokeObjectURL = vi.fn<(url: string) => void>();

  beforeEach(() => {
    vi.resetAllMocks();
    createObjectURL.mockReturnValue("blob:authorized-preview");
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("renders and downloads only an authorized local blob", async () => {
    const image = attachment();
    transfer.read.mockImplementation(
      async (
        _attachment,
        _roomId,
        _authority,
        _mode,
        _signal,
        beforeDispatch?: () => void
      ) => {
        beforeDispatch?.();
        return new Blob(["image"], { type: "image/png" });
      }
    );

    render(
      <LobbyAttachments
        roomId="general"
        authority={{ kind: "local" }}
        attachments={[image]}
      />
    );

    const preview = await screen.findByRole("img", { name: "a.png" });
    expect(preview.getAttribute("src")).toBe("blob:authorized-preview");
    expect(transfer.read).toHaveBeenCalledWith(
      image,
      "general",
      { kind: "local" },
      "view",
      expect.any(AbortSignal),
      expect.any(Function)
    );

    fireEvent.click(screen.getByRole("button", { name: "a.png 크게 보기" }));
    expect(screen.getByRole("dialog", { name: "a.png 이미지 미리보기" })).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "a.png 다운로드" }).getAttribute("href")
    ).toBe("blob:authorized-preview");
  });

  it("aborts a retired room generation before its delayed transfer dispatch", async () => {
    const image = attachment();
    let resolveFirst: ((blob: Blob) => void) | undefined;
    transfer.read
      .mockReturnValueOnce(
        new Promise<Blob>((resolve) => {
          resolveFirst = resolve;
        })
      )
      .mockResolvedValueOnce(new Blob(["image"], { type: "image/png" }));
    const view = render(
      <LobbyAttachments
        roomId="first"
        authority={{ kind: "local" }}
        attachments={[image]}
      />
    );
    await waitFor(() => expect(transfer.read).toHaveBeenCalledOnce());
    const retiredSignal = transfer.read.mock.calls[0]?.[4] as AbortSignal;
    const retiredBeforeDispatch = transfer.read.mock.calls[0]?.[5] as () => void;

    view.rerender(
      <LobbyAttachments
        roomId="second"
        authority={{ kind: "remote", sessionToken: "aas1.session" }}
        attachments={[image]}
      />
    );

    expect(retiredSignal.aborted).toBe(true);
    expect(retiredBeforeDispatch).toThrow();
    resolveFirst?.(new Blob(["image"], { type: "image/png" }));
    await screen.findByRole("img", { name: "a.png" });
    expect(transfer.read).toHaveBeenLastCalledWith(
      image,
      "second",
      { kind: "remote", sessionToken: "aas1.session" },
      "view",
      expect.any(AbortSignal),
      expect.any(Function)
    );
    expect(createObjectURL).toHaveBeenCalledOnce();
  });

  it("revokes generation-owned object URLs on replacement and unmount", async () => {
    transfer.read.mockResolvedValue(new Blob(["bytes"], { type: "text/plain" }));
    createObjectURL
      .mockReturnValueOnce("blob:first")
      .mockReturnValueOnce("blob:second");
    const first = attachment("a", false);
    const second = attachment("b", false);
    const view = render(
      <LobbyAttachments
        roomId="general"
        authority={{ kind: "local" }}
        attachments={[first]}
      />
    );
    await screen.findByRole("link", { name: /a\.txt/ });

    view.rerender(
      <LobbyAttachments
        roomId="general"
        authority={{ kind: "local" }}
        attachments={[second]}
      />
    );
    await screen.findByRole("link", { name: /b\.txt/ });
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:first");

    view.unmount();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:second");
  });

  it("releases already-created URLs when any attachment in the generation fails", async () => {
    const image = attachment("a", true);
    const file = attachment("b", false);
    let rejectFile: ((error: Error) => void) | undefined;
    transfer.read.mockImplementation((value: LobbyAttachmentRef) => {
      if (value.id === image.id) {
        return Promise.resolve(new Blob(["image"], { type: "image/png" }));
      }
      return new Promise<Blob>((_resolve, reject) => {
        rejectFile = reject;
      });
    });
    render(
      <LobbyAttachments
        roomId="general"
        authority={{ kind: "local" }}
        attachments={[image, file]}
      />
    );
    await waitFor(() => expect(createObjectURL).toHaveBeenCalledOnce());
    expect(transfer.read.mock.calls.map((call) => call[3])).toEqual([
      "view",
      "download",
    ]);

    rejectFile?.(new Error("denied"));
    await waitFor(() =>
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:authorized-preview")
    );
    expect(
      transfer.read.mock.calls.every((call) => (call[4] as AbortSignal).aborted)
    ).toBe(true);
    expect(screen.queryByRole("img", { name: "a.png" })).toBeNull();
    expect(screen.queryByRole("link", { name: /b\.txt/ })).toBeNull();
  });
});
