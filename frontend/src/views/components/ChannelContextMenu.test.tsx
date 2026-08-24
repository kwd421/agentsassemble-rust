import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import ChannelContextMenu from "./ChannelContextMenu";

afterEach(cleanup);

describe("ChannelContextMenu preference authority", () => {
  it("blocks local preference changes and exposes the server failure", async () => {
    const onMarkRead = vi.fn();
    const onSetNotifications = vi.fn();
    const onOpenSettings = vi.fn();
    render(
      <ChannelContextMenu
        channelLabel="general"
        x={10}
        y={20}
        onMarkRead={onMarkRead}
        onSetNotifications={onSetNotifications}
        onOpenSettings={onOpenSettings}
        preferenceStatus="error"
        preferenceError="offline"
      />
    );

    expect(screen.getByRole("alert").textContent).toContain("불러오지 못했습니다");
    await userEvent.click(screen.getByRole("menuitem", { name: "읽음으로 표시하기" }));
    await userEvent.click(screen.getByRole("menuitemradio", { name: "모든 메시지" }));
    expect(onMarkRead).not.toHaveBeenCalled();
    expect(onSetNotifications).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("menuitem", { name: "채널 설정" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });
});
