import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useLayoutEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";

import { RoomSocketProvider } from "../../RoomSocketContext";
import {
  RoomSocketSayError,
  type RoomSocketHandle,
} from "../../roomSocketClient";
import LobbyComposer from "./LobbyComposer";

const apiMocks = vi.hoisted(() => ({
  uploadLobbyAttachment: vi.fn(),
}));

vi.mock("../../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../api")>();
  return {
    ...actual,
    uploadLobbyAttachment: apiMocks.uploadLobbyAttachment,
  };
});

describe("LobbyComposer", () => {
  afterEach(() => cleanup());

  beforeEach(() => {
    apiMocks.uploadLobbyAttachment.mockReset();
  });

  it("offers emoji without placeholder gift, GIF, or sticker controls", () => {
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} />);

    expect(screen.queryByLabelText("채팅 선물")).toBeNull();
    expect(screen.queryByLabelText("채팅 GIF")).toBeNull();
    expect(screen.queryByLabelText("채팅 스티커")).toBeNull();
    fireEvent.click(screen.getByLabelText("이모지 삽입"));
    const picker = screen.getByRole("listbox", { name: "이모지 선택" });
    fireEvent.click(within(picker).getByRole("option", { name: "👍" }));

    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("👍");
  });

  it("keeps a message unsent while the canonical socket is unavailable", async () => {
    const onPosted = vi.fn();
    render(
      <LobbyComposer
        meetingId="room-a"
        onPosted={onPosted}
      />
    );

    fireEvent.change(screen.getByLabelText("채팅 입력"), {
      target: { value: "canonical message" },
    });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));

    expect(
      await screen.findByText("방 연결이 준비되지 않았습니다. 연결된 뒤 다시 보내 주세요.")
    ).toBeTruthy();
    await waitFor(() => expect(onPosted).not.toHaveBeenCalled());
  });

  it("keeps the composer focused after an Enter submission finishes", async () => {
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = {
      ready: () => true,
      say,
    } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
      </RoomSocketProvider>
    );

    const input = screen.getByLabelText("채팅 입력") as HTMLTextAreaElement;
    input.focus();
    fireEvent.change(input, { target: { value: "첫 메시지" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => expect(say).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(document.activeElement).toBe(input));
    expect(input.value).toBe("");
  });

  it("uploads attachments from a writable public room session", async () => {
    apiMocks.uploadLobbyAttachment.mockResolvedValue({
      id: "attachment-a",
      filename: "map.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: "/api/rooms/room-a/attachments/attachment-a",
      download_url: "/api/rooms/room-a/attachments/attachment-a/download",
    });
    render(
      <LobbyComposer
        meetingId="room-a"
        onPosted={vi.fn()}
        postingMode="guest"
        roomSessionToken="aas1.public-session"
        roomDeviceToken="browser-device"
      />
    );

    expect(
      (screen.getByLabelText("첨부 추가") as HTMLButtonElement).disabled
    ).toBe(false);
    const file = new File(["map"], "map.png", { type: "image/png" });
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: { files: [file] },
    });

    await waitFor(() =>
      expect(apiMocks.uploadLobbyAttachment).toHaveBeenCalledWith(
        file,
        expect.objectContaining({
          roomId: "room-a",
          sessionToken: "aas1.public-session",
          deviceToken: "browser-device",
          signal: expect.any(AbortSignal),
          beforeDispatch: expect.any(Function),
        })
      )
    );
    expect(await screen.findByText("map.png")).toBeTruthy();
  });

  it("shows a local preview of an image or video before it is sent", async () => {
    const createObjectURL = vi.fn((file: File) => `blob:preview-${file.name}`);
    const revokeObjectURL = vi.fn();
    const original = { createObjectURL: URL.createObjectURL, revokeObjectURL: URL.revokeObjectURL };
    Object.assign(URL, { createObjectURL, revokeObjectURL });
    onTestFinished(() => {
      Object.assign(URL, original);
    });
    const attachment = (id: string, filename: string, contentType: string) => ({
      id,
      filename,
      content_type: contentType,
      size: 3,
      is_image: contentType.startsWith("image/"),
      url: `/api/rooms/room-a/attachments/${id}`,
      download_url: `/api/rooms/room-a/attachments/${id}/download`,
    });
    apiMocks.uploadLobbyAttachment
      .mockResolvedValueOnce(attachment("attachment-a", "map.png", "image/png"))
      .mockResolvedValueOnce(attachment("attachment-b", "clip.mp4", "video/mp4"))
      .mockResolvedValueOnce(attachment("attachment-c", "notes.txt", "text/plain"));
    const { container } = render(
      <LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="host" />
    );

    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: {
        files: [
          new File(["map"], "map.png", { type: "image/png" }),
          new File(["mp4"], "clip.mp4", { type: "video/mp4" }),
          new File(["txt"], "notes.txt", { type: "text/plain" }),
        ],
      },
    });

    expect(await screen.findByText("notes.txt")).toBeTruthy();
    expect(container.querySelector("img[src='blob:preview-map.png']")).toBeTruthy();
    expect(container.querySelector("video[src='blob:preview-clip.mp4']")).toBeTruthy();
    expect(createObjectURL).toHaveBeenCalledTimes(2);

    fireEvent.click(screen.getByLabelText("map.png 첨부 제거"));
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:preview-map.png"));
    expect(container.querySelector("img[src='blob:preview-map.png']")).toBeNull();
  });

  it("clears an attachment error on its own and on request", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    onTestFinished(() => {
      vi.useRealTimers();
    });
    const tooLarge = "메시지 첨부는 1바이트 이상 10MiB 이하여야 합니다.";
    apiMocks.uploadLobbyAttachment.mockRejectedValue(new Error(tooLarge));
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="host" />);
    const pick = () =>
      fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
        target: { files: [new File(["x"], "big.mov", { type: "video/quicktime" })] },
      });

    pick();
    expect(await screen.findByText(tooLarge)).toBeTruthy();
    await act(() => vi.advanceTimersByTimeAsync(5000));
    expect(screen.queryByText(tooLarge)).toBeNull();

    pick();
    expect(await screen.findByText(tooLarge)).toBeTruthy();
    fireEvent.click(screen.getByLabelText("알림 닫기"));
    expect(screen.queryByText(tooLarge)).toBeNull();
  });

  it("attaches a pasted image and leaves a text paste to the input", async () => {
    apiMocks.uploadLobbyAttachment.mockResolvedValue({
      id: "attachment-a",
      filename: "image.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: "/api/rooms/room-a/attachments/attachment-a",
      download_url: "/api/rooms/room-a/attachments/attachment-a/download",
    });
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="host" />);
    const input = screen.getByLabelText("채팅 입력");

    fireEvent.paste(input, { clipboardData: { files: [], getData: () => "plain text" } });
    expect(apiMocks.uploadLobbyAttachment).not.toHaveBeenCalled();

    const image = new File(["png"], "image.png", { type: "image/png" });
    fireEvent.paste(input, { clipboardData: { files: [image], getData: () => "" } });

    await waitFor(() =>
      expect(apiMocks.uploadLobbyAttachment).toHaveBeenCalledWith(
        image,
        expect.objectContaining({ roomId: "room-a" })
      )
    );
    expect(await screen.findByText("image.png")).toBeTruthy();
  });

  it.each([
    ["room", { meetingId: "room-b", postingMode: "guest" as const, roomSessionToken: "aas1.session-a" }],
    ["session", { meetingId: "room-a", postingMode: "guest" as const, roomSessionToken: "aas1.session-b" }],
    ["authority", { meetingId: "room-a", postingMode: "host" as const, roomSessionToken: "aas1.session-a" }],
    ["role", { meetingId: "room-a", postingMode: "guest" as const, roomSessionToken: "aas1.session-a", disabledReason: "읽기 전용" }],
    ["device", { meetingId: "room-a", postingMode: "guest" as const, roomSessionToken: "aas1.session-a", roomDeviceToken: "new-device" }],
    ["unmount", null],
  ])("retires a delayed attachment upload on %s change", async (_label, nextProps) => {
    apiMocks.uploadLobbyAttachment.mockImplementation(
      (_file, options: { signal: AbortSignal }) =>
        new Promise((_resolve, reject) => {
          options.signal.addEventListener(
            "abort",
            () => reject(options.signal.reason),
            { once: true }
          );
        })
    );
    const view = render(
      <LobbyComposer
        meetingId="room-a"
        onPosted={vi.fn()}
        postingMode="guest"
        roomSessionToken="aas1.session-a"
      />
    );
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: { files: [new File(["map"], "map.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(apiMocks.uploadLobbyAttachment).toHaveBeenCalledOnce());
    const options = apiMocks.uploadLobbyAttachment.mock.calls[0]?.[1] as {
      signal: AbortSignal;
      beforeDispatch: () => void;
    };

    if (nextProps) {
      view.rerender(<LobbyComposer {...nextProps} onPosted={vi.fn()} />);
    } else {
      view.unmount();
    }

    await waitFor(() => expect(options.signal.aborted).toBe(true));
    expect(options.beforeDispatch).toThrow();
    if (nextProps) expect(screen.queryByText("map.png")).toBeNull();
  });

  it("retires the previous upload before authority-change layout observers run", async () => {
    apiMocks.uploadLobbyAttachment.mockImplementation(
      (_file, options: { signal: AbortSignal }) =>
        new Promise((_resolve, reject) => {
          options.signal.addEventListener(
            "abort",
            () => reject(options.signal.reason),
            { once: true }
          );
        })
    );
    function LayoutProbe({
      meetingId,
      inspect,
    }: {
      meetingId: string;
      inspect: () => void;
    }) {
      useLayoutEffect(inspect, [inspect, meetingId]);
      return <LobbyComposer meetingId={meetingId} onPosted={vi.fn()} />;
    }
    const view = render(
      <LayoutProbe meetingId="room-a" inspect={() => {}} />
    );
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: { files: [new File(["map"], "map.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(apiMocks.uploadLobbyAttachment).toHaveBeenCalledOnce());
    const options = apiMocks.uploadLobbyAttachment.mock.calls[0]?.[1] as {
      signal: AbortSignal;
      beforeDispatch: () => void;
    };
    const inspect = vi.fn(() => {
      expect(options.signal.aborted).toBe(true);
      expect(options.beforeDispatch).toThrow();
    });

    view.rerender(<LayoutProbe meetingId="room-b" inspect={inspect} />);

    expect(inspect).toHaveBeenCalledOnce();
  });

  it("keeps message and attachment drafts owned by their room", async () => {
    apiMocks.uploadLobbyAttachment.mockResolvedValue({
      id: "attachment-a",
      filename: "map.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: "/api/rooms/room-a/attachments/attachment-a",
      download_url: "/api/rooms/room-a/attachments/attachment-a/download",
    });
    const view = render(
      <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
    );

    fireEvent.change(screen.getByLabelText("채팅 입력"), {
      target: { value: "room A draft" },
    });
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: {
        files: [new File(["map"], "map.png", { type: "image/png" })],
      },
    });
    expect(await screen.findByText("map.png")).toBeTruthy();

    view.rerender(
      <LobbyComposer meetingId="room-b" onPosted={vi.fn()} />
    );
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("");
    expect(screen.queryByText("map.png")).toBeNull();
    fireEvent.change(screen.getByLabelText("채팅 입력"), {
      target: { value: "room B draft" },
    });

    view.rerender(
      <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
    );
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe(
      "room A draft"
    );
    expect(screen.getByText("map.png")).toBeTruthy();
  });

  it("retries the original uncertain draft and creates a new intent only after editing", async () => {
    const retry = vi.fn().mockRejectedValueOnce(new Error("still offline")).mockResolvedValue({});
    const say = vi.fn().mockRejectedValueOnce(new RoomSocketSayError("uncertain", "outcome_unknown", retry))
      .mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(<RoomSocketProvider socket={socket}><LobbyComposer meetingId="room-a" onPosted={vi.fn()} /></RoomSocketProvider>);
    const input = screen.getByLabelText("채팅 입력") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "committed once" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    fireEvent.click(await screen.findByLabelText("같은 요청 다시 보내기"));
    await screen.findByText("still offline");
    expect(say).toHaveBeenCalledTimes(1);
    expect(input.value).toBe("committed once");
    fireEvent.click(screen.getByLabelText("같은 요청 다시 보내기"));
    await waitFor(() => expect(input.value).toBe(""));
    expect(retry).toHaveBeenCalledTimes(2);
    expect(say).toHaveBeenCalledTimes(1);
    say.mockRejectedValueOnce(new RoomSocketSayError("uncertain again", "outcome_unknown", retry));
    fireEvent.change(input, { target: { value: "second uncertain message" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await screen.findByLabelText("같은 요청 다시 보내기");
    fireEvent.change(input, { target: { value: "new edited message" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await waitFor(() => expect(input.value).toBe(""));
    expect(retry).toHaveBeenCalledTimes(2);
    expect(say).toHaveBeenLastCalledWith(expect.objectContaining({ message: "new edited message" }));
  });

  it("keeps text and attachments after the canonical socket rejects the send", async () => {
    const id = `ma_${"a".repeat(32)}`;
    const uploaded = {
      id,
      filename: "map.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: `/api/attachments/${id}?view=1`,
      download_url: `/api/attachments/${id}?download=1`,
    };
    apiMocks.uploadLobbyAttachment.mockResolvedValue(uploaded);
    const say = vi.fn().mockRejectedValue(
      new RoomSocketSayError("attachment unavailable", "attachment_unavailable")
    );
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
      </RoomSocketProvider>
    );

    fireEvent.change(screen.getByLabelText("채팅 입력"), {
      target: { value: "keep this draft" },
    });
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: { files: [new File(["map"], "map.png", { type: "image/png" })] },
    });
    await screen.findByText("map.png");
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));

    expect(await screen.findByText("attachment unavailable")).toBeTruthy();
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe(
      "keep this draft"
    );
    expect(screen.getByText("map.png")).toBeTruthy();
    expect(say).toHaveBeenCalledWith(expect.objectContaining({
      message: "keep this draft",
      attachments: [uploaded],
    }));
  });

  it("keeps attachment upload blocked for a read-only public room session", () => {
    render(
      <LobbyComposer
        meetingId="room-a"
        onPosted={vi.fn()}
        postingMode="guest"
        roomSessionToken="aas1.read-only-session"
        disabledReason="읽기 전용 초대입니다."
      />
    );

    expect(
      (screen.getByLabelText("첨부 추가") as HTMLButtonElement).disabled
    ).toBe(true);
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: {
        files: [new File(["map"], "map.png", { type: "image/png" })],
      },
    });
    expect(apiMocks.uploadLobbyAttachment).not.toHaveBeenCalled();
  });

  it("discovers and opens the vote command without sending chat", async () => {
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = {
      ready: () => true,
      say,
    } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
      </RoomSocketProvider>
    );

    const input = screen.getByLabelText("채팅 입력");
    fireEvent.change(input, { target: { value: "/" } });

    const commandMenu = screen.getByRole("listbox", { name: "채팅 명령" });
    expect(within(commandMenu).getByRole("option").textContent).toContain("/vote");
    expect(input.getAttribute("aria-controls")).toBe(commandMenu.id);
    expect(input.getAttribute("aria-expanded")).toBe("true");
    fireEvent.keyDown(input, { key: "Enter" });

    const dialog = await screen.findByRole("dialog", { name: "투표 만들기" });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "선택지 추가" })
    );
    expect(
      within(dialog).getByRole("textbox", { name: "선택지 3" })
    ).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "선택지 3 제거" })
    );
    expect(
      within(dialog).queryByRole("textbox", { name: "선택지 3" })
    ).toBeNull();
    expect(say).not.toHaveBeenCalled();
  });

  it("contains modal focus, restores the composer focus, and closes on room change", async () => {
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = {
      ready: () => true,
      say,
    } as unknown as RoomSocketHandle;
    const view = render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} />
      </RoomSocketProvider>
    );

    const composer = screen.getByLabelText("채팅 입력") as HTMLTextAreaElement;
    composer.focus();
    fireEvent.change(composer, { target: { value: "/vote" } });
    fireEvent.keyDown(composer, { key: "Enter" });
    const dialog = await screen.findByRole("dialog", { name: "투표 만들기" });
    const first = within(dialog).getByRole("button", {
      name: "투표 만들기 닫기",
    });
    const last = within(dialog).getByRole("button", { name: "만들기" });

    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);

    fireEvent.click(within(dialog).getByRole("button", { name: "취소" }));
    expect(document.activeElement).toBe(composer);

    fireEvent.keyDown(composer, { key: "Enter" });
    expect(
      await screen.findByRole("dialog", { name: "투표 만들기" })
    ).toBeTruthy();
    view.rerender(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-b" onPosted={vi.fn()} />
      </RoomSocketProvider>
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "투표 만들기" })
      ).toBeNull()
    );
    expect(say).not.toHaveBeenCalled();
  });

  it("retains the uncertain vote request across retry failures", async () => {
    const retry = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue({});
    const say = vi.fn().mockRejectedValueOnce(new RoomSocketSayError("uncertain vote", "outcome_unknown", retry)).mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(<RoomSocketProvider socket={socket}><LobbyComposer meetingId="room-a" onPosted={vi.fn()} /></RoomSocketProvider>);
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "/vote" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    const dialog = await screen.findByRole("dialog", { name: "투표 만들기" });
    for (const [name, value] of [["질문", "One vote"], ["선택지 1", "A"], ["선택지 2", "B"]])
      fireEvent.change(within(dialog).getByRole("textbox", { name }), { target: { value } });
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));
    await within(dialog).findByText("uncertain vote");
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));
    await within(dialog).findByText("offline");
    expect(say).toHaveBeenCalledTimes(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "투표 만들기" })).toBeNull());
    expect(retry).toHaveBeenCalledTimes(2);
    expect(say).toHaveBeenCalledTimes(1);
  });

  it("submits the validated vote and clears its staged attachment", async () => {
    const id = `ma_${"a".repeat(32)}`;
    const uploaded = {
      id,
      filename: "map.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: `/api/attachments/${id}?view=1`,
      download_url: `/api/attachments/${id}?download=1`,
    };
    apiMocks.uploadLobbyAttachment.mockResolvedValue(uploaded);
    const say = vi.fn().mockResolvedValue({ events: [] });
    const onPosted = vi.fn();
    const socket = {
      ready: () => true,
      say,
    } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={onPosted} />
      </RoomSocketProvider>
    );

    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), {
      target: { files: [new File(["map"], "map.png", { type: "image/png" })] },
    });
    await screen.findByText("map.png");

    fireEvent.change(screen.getByLabelText("채팅 입력"), {
      target: { value: "/vote" },
    });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    const dialog = await screen.findByRole("dialog", { name: "투표 만들기" });

    expect(
      (within(dialog).getByRole("button", {
        name: "선택지 1 제거",
      }) as HTMLButtonElement).disabled
    ).toBe(true);
    fireEvent.change(within(dialog).getByRole("textbox", { name: "질문" }), {
      target: { value: "어느 길로 갈까요?" },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "선택지 1" }), {
      target: { value: "북쪽" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "만들기" })
    );

    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "모든 선택지에 이름을 입력해 주세요."
    );
    expect(say).not.toHaveBeenCalled();

    fireEvent.change(within(dialog).getByRole("textbox", { name: "선택지 2" }), {
      target: { value: "남쪽" },
    });
    fireEvent.change(
      within(dialog).getByRole("spinbutton", { name: "투표 기간 (분)" }),
      { target: { value: "15" } }
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "만들기" })
    );

    await waitFor(() =>
      expect(say).toHaveBeenCalledWith({
        message: "",
        attachments: [uploaded],
        kind: "vote",
        voteQuestion: "어느 길로 갈까요?",
        voteOptions: ["북쪽", "남쪽"],
        voteDurationSeconds: 900,
      })
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "투표 만들기" })).toBeNull()
    );
    expect(screen.queryByText("map.png")).toBeNull();
    expect(onPosted).toHaveBeenCalledWith([]);
  });
});
