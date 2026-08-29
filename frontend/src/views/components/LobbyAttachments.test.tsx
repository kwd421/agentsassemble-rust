import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLayoutEffect, useMemo, useState } from "react";

const transfer = vi.hoisted(() => ({ read: vi.fn() }));

vi.mock("../../api/messageAttachments", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/messageAttachments")>()),
  fetchMessageAttachmentBlob: transfer.read,
}));

import type { LobbyAttachmentRef } from "../../api/messageAttachments";
import { createMessageAttachmentReadOwner } from "../../lib/messageAttachmentReadScheduler";
import LobbyAttachments from "./LobbyAttachments";

const intersectionObservers: TestIntersectionObserver[] = [];

class TestIntersectionObserver {
  private readonly callback: IntersectionObserverCallback;
  private target: Element | null = null;

  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback;
    intersectionObservers.push(this);
  }

  observe(target: Element) {
    this.target = target;
  }

  disconnect() {
    this.target = null;
  }

  emit(isIntersecting: boolean) {
    if (!this.target) throw new Error("observer target is unavailable");
    this.callback(
      [{ isIntersecting, target: this.target } as IntersectionObserverEntry],
      this as unknown as IntersectionObserver
    );
  }
}

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

function scheduler() {
  return createMessageAttachmentReadOwner(transfer.read).forAuthority(
    "general",
    { kind: "local" }
  );
}

describe("LobbyAttachments", () => {
  const createObjectURL = vi.fn<(blob: Blob) => string>();
  const revokeObjectURL = vi.fn<(url: string) => void>();
  let anchorClick: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.resetAllMocks();
    intersectionObservers.length = 0;
    vi.stubGlobal("IntersectionObserver", TestIntersectionObserver);
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
    anchorClick = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  });

  afterEach(() => {
    cleanup();
    anchorClick.mockRestore();
    vi.unstubAllGlobals();
  });

  it("reads only an intersecting image or an explicitly downloaded file", async () => {
    const image = attachment("a", true);
    const file = attachment("b", false);
    createObjectURL
      .mockReturnValueOnce("blob:image")
      .mockReturnValueOnce("blob:file");
    transfer.read.mockImplementation(
      async (value: LobbyAttachmentRef, ...args: unknown[]) => {
        const beforeDispatch = args[4] as (() => void) | undefined;
        beforeDispatch?.();
        return new Blob([value.is_image ? "image" : "file"], {
          type: value.content_type,
        });
      }
    );
    render(
      <LobbyAttachments
        attachments={[image, file]}
        scheduler={scheduler()}
      />
    );

    expect(transfer.read).not.toHaveBeenCalled();
    act(() => intersectionObservers[0]?.emit(true));
    const preview = await screen.findByRole("img", { name: "a.png" });
    expect(preview.getAttribute("src")).toBe("blob:image");
    expect(transfer.read).toHaveBeenCalledWith(
      image,
      "general",
      { kind: "local" },
      "view",
      expect.any(AbortSignal),
      expect.any(Function)
    );

    fireEvent.click(screen.getByRole("button", { name: "b.txt 다운로드" }));
    await waitFor(() => expect(anchorClick).toHaveBeenCalledOnce());
    expect(transfer.read).toHaveBeenLastCalledWith(
      file,
      "general",
      { kind: "local" },
      "download",
      expect.any(AbortSignal),
      expect.any(Function)
    );
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:file");

    fireEvent.click(screen.getByRole("button", { name: "a.png 크게 보기" }));
    expect(screen.getByRole("dialog", { name: "a.png 이미지 미리보기" })).toBeTruthy();
  });

  it("keeps reads available after the production StrictMode effect reconnect", async () => {
    const image = attachment("a", true);
    const file = attachment("b", false);
    createObjectURL
      .mockReturnValueOnce("blob:strict-image")
      .mockReturnValueOnce("blob:strict-file");
    transfer.read.mockImplementation(
      async (value: LobbyAttachmentRef) =>
        new Blob([value.filename], { type: value.content_type })
    );
    function StrictReader() {
      const [owner] = useState(() => createMessageAttachmentReadOwner(transfer.read));
      const reader = useMemo(
        () => owner.forAuthority("general", { kind: "local" }),
        [owner]
      );
      return <LobbyAttachments attachments={[image, file]} scheduler={reader} />;
    }
    render(<StrictReader />, { reactStrictMode: true });

    act(() => intersectionObservers.at(-1)?.emit(true));
    expect(await screen.findByRole("img", { name: "a.png" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "b.txt 다운로드" }));
    await waitFor(() => expect(anchorClick).toHaveBeenCalledOnce());
    expect(transfer.read).toHaveBeenCalledTimes(2);
  });

  it("aborts and revokes an image as soon as it leaves the viewport", async () => {
    const image = attachment();
    createObjectURL.mockReturnValue("blob:image");
    transfer.read.mockResolvedValue(new Blob(["image"], { type: "image/png" }));
    const view = render(
      <LobbyAttachments attachments={[image]} scheduler={scheduler()} />
    );
    act(() => intersectionObservers[0]?.emit(true));
    await screen.findByRole("img", { name: "a.png" });

    act(() => intersectionObservers[0]?.emit(false));
    await waitFor(() => expect(screen.queryByRole("img", { name: "a.png" })).toBeNull());
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:image");

    transfer.read.mockReturnValueOnce(new Promise<Blob>(() => {}));
    act(() => intersectionObservers[0]?.emit(true));
    await waitFor(() => expect(transfer.read).toHaveBeenCalledTimes(2));
    const activeSignal = transfer.read.mock.calls[1]?.[4] as AbortSignal;
    act(() => intersectionObservers[0]?.emit(false));
    expect(activeSignal.aborted).toBe(true);

    transfer.read.mockResolvedValueOnce(new Blob(["image"], { type: "image/png" }));
    act(() => intersectionObservers[0]?.emit(true));
    await screen.findByRole("img", { name: "a.png" });
    view.unmount();
    expect(revokeObjectURL).toHaveBeenCalledTimes(2);
  });

  it("revokes the previous authority image before layout observers run", async () => {
    const image = attachment();
    const owner = createMessageAttachmentReadOwner(transfer.read);
    const firstScheduler = owner.forAuthority("room-a", { kind: "local" });
    const secondScheduler = owner.forAuthority(
      "room-b",
      { kind: "remote", sessionToken: "aas1.next" }
    );
    createObjectURL.mockReturnValueOnce("blob:first-authority");
    transfer.read
      .mockResolvedValueOnce(new Blob(["image"], { type: "image/png" }))
      .mockReturnValueOnce(new Promise<Blob>(() => {}));
    function LayoutProbe({
      activeScheduler,
      inspect,
    }: {
      activeScheduler: ReturnType<typeof scheduler>;
      inspect: () => void;
    }) {
      useLayoutEffect(inspect, [activeScheduler, inspect]);
      return (
        <LobbyAttachments
          attachments={[image]}
          scheduler={activeScheduler}
        />
      );
    }
    const view = render(
      <LayoutProbe activeScheduler={firstScheduler} inspect={() => {}} />
    );
    act(() => intersectionObservers[0]?.emit(true));
    await screen.findByRole("img", { name: "a.png" });
    const inspect = vi.fn(() => {
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:first-authority");
    });

    view.rerender(
      <LayoutProbe activeScheduler={secondScheduler} inspect={inspect} />
    );

    expect(inspect).toHaveBeenCalledOnce();
    await waitFor(() =>
      expect(screen.queryByRole("img", { name: "a.png" })).toBeNull()
    );
  });

  it("isolates one image failure and retries only that item", async () => {
    const first = attachment("a", true);
    const second = attachment("b", true);
    let firstAttempt = true;
    createObjectURL
      .mockReturnValueOnce("blob:second")
      .mockReturnValueOnce("blob:first-retry");
    transfer.read.mockImplementation(async (value: LobbyAttachmentRef) => {
      if (value.id === first.id && firstAttempt) {
        firstAttempt = false;
        throw new Error("denied");
      }
      return new Blob([value.filename], { type: "image/png" });
    });
    render(
      <LobbyAttachments attachments={[first, second]} scheduler={scheduler()} />
    );

    act(() => {
      intersectionObservers[0]?.emit(true);
      intersectionObservers[1]?.emit(true);
    });
    await screen.findByRole("img", { name: "b.png" });
    const retry = await screen.findByRole("button", {
      name: "a.png 미리보기 다시 시도",
    });
    expect(screen.getByRole("img", { name: "b.png" })).toBeTruthy();

    fireEvent.click(retry);
    await screen.findByRole("img", { name: "a.png" });
    expect(transfer.read.mock.calls.map((call) => call[0].id)).toEqual([
      first.id,
      second.id,
      first.id,
    ]);
    expect(screen.getByRole("img", { name: "b.png" })).toBeTruthy();
  });
});
