import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, onTestFinished, vi } from "vitest";

import { RoomSocketProvider } from "../../RoomSocketContext";
import {
  RoomSocketSayError,
  type RoomSocketHandle,
} from "../../roomSocketClient";
import LobbyComposer, { type ComposerOutgoing } from "./LobbyComposer";

describe("LobbyComposer", () => {
  it("sends the selected reply and does not reuse its uncertain request after changing the source", async () => {
    const retry = vi.fn().mockResolvedValue(undefined);
    const uncertain = new RoomSocketSayError("uncertain", "uncertain", retry);
    const say = vi.fn().mockRejectedValueOnce(uncertain).mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    const onReplySent = vi.fn();
    const view = (id: string) => <RoomSocketProvider socket={socket}><LobbyComposer meetingId="room-a" onPosted={vi.fn()} replyTo={{eventId:id, author:"Host", text:"source"}} onReplySent={onReplySent} /></RoomSocketProvider>;
    const { rerender } = render(view("first"));
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "reply" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await screen.findByText("채팅 메시지 전송 실패");
    rerender(view("second"));
    fireEvent.click(screen.getByLabelText("같은 요청 다시 보내기"));
    await waitFor(() => expect(onReplySent).toHaveBeenCalledWith("second"));
    expect(retry).not.toHaveBeenCalled();
    expect(say).toHaveBeenLastCalledWith(expect.objectContaining({ message: "reply", replyToEventId: "second" }));
  });
  afterEach(() => cleanup());

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

  it("reuses tool permissions and returns focus when the compact menu closes", () => {
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="guest" />);
    const toggle = screen.getByRole("button", { name: "채팅 도구 열기" });
    fireEvent.click(toggle);
    const menu = screen.getByRole("menu", { name: "채팅 도구" });
    expect(within(menu).getByRole("menuitem", { name: "첨부 추가" })).toHaveProperty("disabled", true);
    expect(within(menu).getByRole("menuitem", { name: "채팅 앱" })).toBe(document.activeElement);
    fireEvent.keyDown(document.activeElement!, { key: "End" });
    expect(within(menu).getByRole("menuitem", { name: "이모지 삽입" })).toBe(document.activeElement);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(toggle);
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "/vote" } });
    fireEvent.click(toggle);
    expect(screen.queryByRole("listbox", { name: "채팅 명령" })).toBeNull();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
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
      await screen.findByText("방 연결이 준비되지 않았어요. 연결된 뒤 다시 보내 주세요.")
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
    await screen.findByText("채팅 메시지 전송 실패");
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
    await within(dialog).findByText("투표를 만들지 못했어요.");
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));
    await within(dialog).findByText("투표를 만들지 못했어요.");
    expect(say).toHaveBeenCalledTimes(1);
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "투표 만들기" })).toBeNull());
    expect(retry).toHaveBeenCalledTimes(2);
    expect(say).toHaveBeenCalledTimes(1);
  });

  // Files are staged locally and travel with the message; the fake stands in for the
  // room's outgoing-message owner.
  function outgoingFake(overrides: Partial<ComposerOutgoing> = {}): ComposerOutgoing {
    return { ready: true, limit: 10, check: () => "", send: vi.fn(), ...overrides };
  }
  function pick(...files: File[]) {
    fireEvent.change(screen.getByLabelText("채팅 첨부 선택"), { target: { files } });
  }
  const png = (name = "map.png") => new File(["map"], name, { type: "image/png" });

  it("stages picked files without sending anything and lets one be removed", async () => {
    const outgoing = outgoingFake();
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />);

    pick(png(), new File(["txt"], "notes.txt", { type: "text/plain" }));
    expect(await screen.findByText("map.png")).toBeTruthy();
    expect(screen.getByText("notes.txt")).toBeTruthy();
    expect(outgoing.send).not.toHaveBeenCalled();

    fireEvent.click(screen.getByLabelText("map.png 첨부 제거"));
    expect(screen.queryByText("map.png")).toBeNull();
    expect(screen.getByText("notes.txt")).toBeTruthy();
  });

  it("refuses a selection that does not fit in one message instead of attaching part of it", async () => {
    HTMLDialogElement.prototype.showModal ??= function showModal(this: HTMLDialogElement) { this.open = true; };
    HTMLDialogElement.prototype.close ??= function close(this: HTMLDialogElement) { this.open = false; };
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoingFake({ limit: 8 })} />);

    pick(...Array.from({ length: 9 }, (_, index) => new File(["x"], `${index}.txt`, { type: "text/plain" })));
    fireEvent.click(await screen.findByRole("button", { name: "확인", hidden: true }));
    expect(screen.queryByText("0.txt")).toBeNull();
  });

  it("refuses a selection with a file the room cannot take, and the notice clears on its own", async () => {
    vi.useFakeTimers();
    onTestFinished(() => { vi.useRealTimers(); });
    const tooLarge = "파일은 최대 10MB까지 올릴 수 있어요.";
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()}
      outgoing={outgoingFake({ check: (file) => file.name === "big.bin" ? tooLarge : "" })} />);

    pick(png(), new File(["x"], "big.bin"));
    expect(screen.getByText(tooLarge)).toBeTruthy();
    expect(screen.queryByText("map.png")).toBeNull();
    act(() => { vi.advanceTimersByTime(6000); });
    expect(screen.queryByText(tooLarge)).toBeNull();
  });

  it("shows a local preview of an image or video before it is sent", async () => {
    const createObjectURL = vi.fn((file: File) => `blob:preview-${file.name}`);
    const revokeObjectURL = vi.fn();
    const original = { createObjectURL: URL.createObjectURL, revokeObjectURL: URL.revokeObjectURL };
    Object.assign(URL, { createObjectURL, revokeObjectURL });
    onTestFinished(() => {
      Object.assign(URL, original);
    });
    const { container } = render(
      <LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="host" outgoing={outgoingFake()} />
    );

    pick(png(), new File(["mp4"], "clip.mp4", { type: "video/mp4" }), new File(["txt"], "notes.txt", { type: "text/plain" }));

    expect(await screen.findByText("notes.txt")).toBeTruthy();
    expect(container.querySelector("img[src='blob:preview-map.png']")).toBeTruthy();
    expect(container.querySelector("video[src='blob:preview-clip.mp4']")).toBeTruthy();
    expect(createObjectURL).toHaveBeenCalledTimes(2);

    fireEvent.click(screen.getByLabelText("map.png 첨부 제거"));
    await waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith("blob:preview-map.png"));
    expect(container.querySelector("img[src='blob:preview-map.png']")).toBeNull();
  });

  it("stages a pasted image and leaves a text paste to the input", async () => {
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoingFake()} />);
    const input = screen.getByLabelText("채팅 입력");

    expect(fireEvent.paste(input, { clipboardData: { files: [] } })).toBe(true);
    expect(fireEvent.paste(input, { clipboardData: { files: [png("shot.png")] } })).toBe(false);
    expect(await screen.findByText("shot.png")).toBeTruthy();
  });

  it("keeps message and staged files owned by their room", async () => {
    const outgoing = outgoingFake();
    const { rerender } = render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />);
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "for room a" } });
    pick(png());
    await screen.findByText("map.png");

    rerender(<LobbyComposer meetingId="room-b" onPosted={vi.fn()} outgoing={outgoing} />);
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("");
    expect(screen.queryByText("map.png")).toBeNull();

    rerender(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />);
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("for room a");
    expect(screen.getByText("map.png")).toBeTruthy();
  });

  it("hands a message with files to the outgoing owner and frees the composer at once", async () => {
    const outgoing = outgoingFake();
    const say = vi.fn();
    const onReplySent = vi.fn();
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing}
          replyTo={{ eventId: "source", author: "Host", text: "hi" }} onReplySent={onReplySent} />
      </RoomSocketProvider>
    );
    const file = png();
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: " here it is " } });
    pick(file);
    await screen.findByText("map.png");
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));

    await waitFor(() => expect(outgoing.send).toHaveBeenCalledTimes(1));
    expect(outgoing.send).toHaveBeenCalledWith({ text: "here it is", replyToEventId: "source", files: [file], request: undefined });
    expect(say).not.toHaveBeenCalled();
    expect(onReplySent).toHaveBeenCalledWith("source");
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("");
    expect(screen.queryByText("map.png")).toBeNull();
  });

  it("starts a reply with Discord's @ on, sends it, and offers no switch to hosts without it", async () => {
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    const reply = { eventId: "source", author: "Host", text: "hi" };
    const { rerender } = render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} replyTo={reply} onCancelReply={vi.fn()} replyMentionSupported />
      </RoomSocketProvider>
    );
    const toggle = screen.getByRole("button", { name: "@ 켜짐" });
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "calling" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await waitFor(() => expect(say).toHaveBeenCalledTimes(1));
    expect(say.mock.calls[0][0]).toMatchObject({ replyToEventId: "source", replyMention: true });

    fireEvent.click(screen.getByRole("button", { name: "@ 켜짐" }));
    expect(screen.getByRole("button", { name: "@ 꺼짐" }).getAttribute("aria-pressed")).toBe("false");
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "quiet" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await waitFor(() => expect(say).toHaveBeenCalledTimes(2));
    expect(say.mock.calls[1][0]).toMatchObject({ replyToEventId: "source", replyMention: false });

    rerender(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} replyTo={reply} onCancelReply={vi.fn()} />
      </RoomSocketProvider>
    );
    expect(screen.queryByRole("button", { name: /^@ / })).toBeNull();
  });

  it("keeps the text and the staged files when the outgoing owner refuses the message", async () => {
    const outgoing = outgoingFake({ send: vi.fn(() => { throw new Error("잠시 후 다시 시도해 주세요."); }) });
    render(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />);
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "keep this draft" } });
    pick(png());
    await screen.findByText("map.png");
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));

    expect(await screen.findByText("잠시 후 다시 시도해 주세요.")).toBeTruthy();
    expect((screen.getByLabelText("채팅 입력") as HTMLTextAreaElement).value).toBe("keep this draft");
    expect(screen.getByText("map.png")).toBeTruthy();
  });

  it("keeps attaching blocked for a read-only session and until the room's owner is ready", () => {
    const outgoing = outgoingFake();
    const { rerender } = render(
      <LobbyComposer meetingId="room-a" onPosted={vi.fn()} postingMode="guest"
        roomSessionToken="aas1.read-only-session" disabledReason="읽기 전용 초대예요." outgoing={outgoing} />
    );
    expect((screen.getByLabelText("첨부 추가") as HTMLButtonElement).disabled).toBe(true);
    pick(png());
    expect(screen.queryByText("map.png")).toBeNull();

    rerender(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoingFake({ ready: false })} />);
    expect((screen.getByLabelText("첨부 추가") as HTMLButtonElement).disabled).toBe(true);
    rerender(<LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />);
    expect((screen.getByLabelText("첨부 추가") as HTMLButtonElement).disabled).toBe(false);
  });

  it("sends a vote's staged files with the vote through the outgoing owner", async () => {
    const outgoing = outgoingFake();
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()} outgoing={outgoing} />
      </RoomSocketProvider>
    );
    const file = png();
    pick(file);
    await screen.findByText("map.png");
    fireEvent.change(screen.getByLabelText("채팅 입력"), { target: { value: "/vote" } });
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    const dialog = await screen.findByRole("dialog", { name: "투표 만들기" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "질문" }), { target: { value: "어느 길로 갈까요?" } });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "선택지 1" }), { target: { value: "북쪽" } });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "선택지 2" }), { target: { value: "남쪽" } });
    fireEvent.change(within(dialog).getByRole("spinbutton", { name: "투표 기간 (분)" }), { target: { value: "15" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "만들기" }));

    await waitFor(() => expect(outgoing.send).toHaveBeenCalledWith({
      text: "", files: [file],
      request: { kind: "vote", voteQuestion: "어느 길로 갈까요?", voteOptions: ["북쪽", "남쪽"], voteDurationSeconds: 900 },
    }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "투표 만들기" })).toBeNull());
    expect(say).not.toHaveBeenCalled();
    expect(screen.queryByText("map.png")).toBeNull();
  });

  it("binds a picked mention to the participant id and sends it unchanged", async () => {
    const say = vi.fn().mockResolvedValue({ events: [] });
    const socket = { ready: () => true, say } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyComposer meetingId="room-a" onPosted={vi.fn()}
          mentionables={[{ token: "opencode-87a106c9-test", label: "claude-haiku-5-5", participantKind: "agent" }]} />
      </RoomSocketProvider>
    );
    const input = screen.getByLabelText("채팅 입력") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "@cl" } });
    input.selectionStart = input.value.length;
    fireEvent.keyUp(input);
    fireEvent.click(await screen.findByRole("option", { name: "claude-haiku-5-5" }));
    // The pick carries the participant id through send, so a duplicate or
    // normalized-equal display name never resolves to another participant.
    expect(input.value).toBe("<@opencode-87a106c9-test> ");
    fireEvent.click(screen.getByLabelText("채팅 메시지 보내기"));
    await waitFor(() => expect(say).toHaveBeenCalledTimes(1));
    expect(say).toHaveBeenCalledWith(expect.objectContaining({ message: "<@opencode-87a106c9-test>" }));
  });
});
