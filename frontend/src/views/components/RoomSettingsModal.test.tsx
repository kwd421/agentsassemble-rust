import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Radio } from "lucide-react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_ROOM_APPEARANCE } from "../../lib/roomAppearance";
import RoomSettingsModal from "./RoomSettingsModal";

afterEach(cleanup);

const room = {
  id: "server-general",
  label: "General",
  meetingId: "general",
  topic: "",
  shortLabel: "G",
  icon: Radio,
  createdAt: "2026-07-14T00:00:00Z",
  tone: "resident" as const,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function renderSettings(
  conversationMode: "ordered" | "ambient" | null,
  onConversationModeChange = vi.fn(),
  settingsStatus: "loading" | "ready" | "saving" | "stale" | "error" = "ready",
  onRetrySettings = vi.fn(),
  onOrderedExcludePreviousSpeakerChange = vi.fn(),
  onAppearanceChange = vi.fn().mockResolvedValue(undefined),
  onToolModeChange = vi.fn(),
  onAppearanceUpload = vi.fn().mockResolvedValue(true)
) {
  render(
    <RoomSettingsModal
      room={room}
      appearance={DEFAULT_ROOM_APPEARANCE}
      appearanceAssetError=""
      channelSettings={{}}
      settingsStatus={settingsStatus}
      settingsError={settingsStatus === "error" ? "offline" : ""}
      preferenceStatus="ready"
      preferenceError=""
      conversationMode={conversationMode}
      toolMode={conversationMode ? "chat" : null}
      orderedExcludePreviousSpeaker={conversationMode ? true : null}
      canInvite
      onClose={() => undefined}
      onInvite={() => undefined}
      onRoomChange={() => undefined}
      onAppearanceChange={onAppearanceChange}
      onAppearanceUpload={onAppearanceUpload}
      onChannelSettingChange={async () => undefined}
      onConversationModeChange={onConversationModeChange}
      onToolModeChange={onToolModeChange}
      onOrderedExcludePreviousSpeakerChange={onOrderedExcludePreviousSpeakerChange}
      onRetrySettings={onRetrySettings}
      onRetryAppearance={() => undefined}
    />
  );
  return { onAppearanceUpload, onConversationModeChange };
}

describe("RoomSettingsModal conversation mode", () => {
  beforeEach(() => vi.resetAllMocks());

  it("does not advertise room deletion before the server owns that action", () => {
    renderSettings("ordered");

    expect(screen.queryByText("서버 삭제")).toBeNull();
    expect(screen.queryByRole("button", { name: "서버 영구 삭제" })).toBeNull();
  });

  it("binds a banner upload to the room and waits for canonical appearance persistence", async () => {
    const onAppearanceChange = vi.fn().mockResolvedValue(undefined);
    const onAppearanceUpload = vi.fn().mockResolvedValue(true);
    renderSettings(
      "ordered",
      vi.fn(),
      "ready",
      vi.fn(),
      vi.fn(),
      onAppearanceChange,
      vi.fn(),
      onAppearanceUpload
    );

    const file = new File(["png"], "banner.png", { type: "image/png" });
    await userEvent.upload(screen.getByLabelText("배너 이미지"), file);

    expect(onAppearanceUpload).toHaveBeenCalledWith(file, "banner");
    expect(onAppearanceChange).not.toHaveBeenCalled();
    expect(await screen.findByText("배너 이미지 저장됨")).toBeTruthy();
  });

  it("sends an explicit empty banner reference when a preset clears the image", async () => {
    const onAppearanceChange = vi.fn().mockResolvedValue(undefined);
    renderSettings("ordered", vi.fn(), "ready", vi.fn(), vi.fn(), onAppearanceChange);

    await userEvent.click(screen.getByRole("button", { name: "그린" }));

    expect(onAppearanceChange).toHaveBeenCalledWith({
      bannerPreset: "forest",
      bannerImage: "",
    });
  });

  it("does not let a superseded upload completion overwrite current status", async () => {
    const first = deferred<boolean>();
    const second = deferred<boolean>();
    const onAppearanceUpload = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    renderSettings(
      "ordered",
      vi.fn(),
      "ready",
      vi.fn(),
      vi.fn(),
      vi.fn().mockResolvedValue(undefined),
      vi.fn(),
      onAppearanceUpload
    );

    await userEvent.upload(
      screen.getByLabelText("배너 이미지"),
      new File(["first"], "first.png", { type: "image/png" })
    );
    await userEvent.upload(
      screen.getByLabelText("배너 이미지"),
      new File(["second"], "second.png", { type: "image/png" })
    );
    second.resolve(true);
    expect(await screen.findByText("배너 이미지 저장됨")).toBeTruthy();
    first.resolve(false);
    await Promise.resolve();

    expect(screen.getByText("배너 이미지 저장됨")).toBeTruthy();
  });

  it("lets the user activate ambient discussion from the room settings UI", async () => {
    const { onConversationModeChange } = renderSettings("ordered");

    await userEvent.click(screen.getByRole("radio", { name: /자유 토론/ }));

    expect(onConversationModeChange).toHaveBeenCalledWith("ambient");
  });

  it("lets the host enable tabletop tools independently of conversation order", async () => {
    const onToolModeChange = vi.fn();
    renderSettings(
      "ordered",
      vi.fn(),
      "ready",
      vi.fn(),
      vi.fn(),
      vi.fn().mockResolvedValue(undefined),
      onToolModeChange
    );

    await userEvent.click(screen.getByRole("radio", { name: /테이블탑/ }));

    expect(onToolModeChange).toHaveBeenCalledWith("tabletop");
  });

  it("lets the user allow the previous speaker in general ordered selection", async () => {
    const onChange = vi.fn();
    renderSettings("ordered", vi.fn(), "ready", vi.fn(), onChange);

    await userEvent.click(
      screen.getByRole("checkbox", { name: /직전 발언자 연속 선택 방지/ })
    );

    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("does not guess routing settings when the server read fails", async () => {
    const onConversationModeChange = vi.fn();
    const onRetrySettings = vi.fn();
    renderSettings(null, onConversationModeChange, "error", onRetrySettings);

    const ordered = screen.getByRole("radio", { name: /순서/ }) as HTMLInputElement;
    expect(ordered.checked).toBe(false);
    expect(ordered.disabled).toBe(true);
    expect(screen.getByRole("alert").textContent).toContain("확인할 수 없어");

    await userEvent.click(screen.getByRole("button", { name: "다시 불러오기" }));

    expect(onRetrySettings).toHaveBeenCalledTimes(1);
    expect(onConversationModeChange).not.toHaveBeenCalled();
  });

  it("offers channel notification controls only for current navigable room channels", async () => {
    const onChannelSettingChange = vi.fn().mockResolvedValue(undefined);
    render(
      <RoomSettingsModal
        room={room}
        appearance={DEFAULT_ROOM_APPEARANCE}
        appearanceAssetError=""
        channelSettings={{}}
        settingsStatus="ready"
        settingsError=""
        preferenceStatus="ready"
        preferenceError=""
        conversationMode="ordered"
        toolMode="chat"
        orderedExcludePreviousSpeaker
        canInvite
        onClose={() => undefined}
        onInvite={() => undefined}
        onRoomChange={() => undefined}
        onAppearanceChange={async () => undefined}
        onAppearanceUpload={async () => true}
        onChannelSettingChange={onChannelSettingChange}
        onConversationModeChange={() => undefined}
        onToolModeChange={() => undefined}
        onOrderedExcludePreviousSpeakerChange={() => undefined}
        onRetrySettings={() => undefined}
        onRetryAppearance={() => undefined}
      />
    );

    const section = screen.getByRole("heading", { name: "채널 설정" }).closest("section");
    if (!section) throw new Error("Channel settings section was not rendered");
    const channelControls = within(section).getAllByRole("combobox");
    expect(channelControls).toHaveLength(1);

    await userEvent.selectOptions(channelControls[0], "mentions");
    expect(onChannelSettingChange).toHaveBeenCalledWith("lobby", {
      notifications: "mentions",
    });
  });

  it("keeps notification controls disabled when preference authority is unavailable", () => {
    render(
      <RoomSettingsModal
        room={room}
        appearance={DEFAULT_ROOM_APPEARANCE}
        appearanceAssetError=""
        channelSettings={{}}
        settingsStatus="ready"
        settingsError=""
        preferenceStatus="error"
        preferenceError="Rust 초대·세션 권한이 아직 연결되지 않았습니다."
        conversationMode="ordered"
        toolMode="chat"
        orderedExcludePreviousSpeaker
        canInvite={false}
        onClose={() => undefined}
        onInvite={() => undefined}
        onRoomChange={() => undefined}
        onAppearanceChange={async () => undefined}
        onAppearanceUpload={async () => true}
        onChannelSettingChange={async () => undefined}
        onConversationModeChange={() => undefined}
        onToolModeChange={() => undefined}
        onOrderedExcludePreviousSpeakerChange={() => undefined}
        onRetrySettings={() => undefined}
        onRetryAppearance={() => undefined}
      />
    );

    expect((screen.getByRole("combobox") as HTMLSelectElement).disabled).toBe(true);
    for (const radio of screen.getAllByRole("radio", { name: /모든 메시지|@멘션만|알림 끔/ })) {
      expect((radio as HTMLInputElement).disabled).toBe(true);
    }
    expect(screen.getAllByRole("alert")[0]?.textContent).toContain("확인할 수 없어");
  });
});
