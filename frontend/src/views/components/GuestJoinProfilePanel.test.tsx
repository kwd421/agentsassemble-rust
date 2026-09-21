import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

vi.mock("./ImageCropper", () => ({
  default: ({
    file,
    onCropped,
  }: {
    file: File;
    onCropped: (file: File) => void;
  }) => (
    <button type="button" onClick={() => onCropped(file)}>
      테스트 이미지 적용
    </button>
  ),
}));

describe("GuestJoinProfilePanel", () => {
  afterEach(cleanup);

  it("keeps the cropped avatar in the browser until admission", async () => {
    const onAvatarImageChange = vi.fn();
    const croppedFile = new File(["avatar"], "avatar.png", { type: "image/png" });

    render(
      <GuestJoinProfilePanel
        displayName="Guest"
        onDisplayNameChange={vi.fn()}
        onAvatarImageChange={onAvatarImageChange}
        onJoin={vi.fn()}
      />
    );

    fireEvent.change(screen.getByLabelText("프로필 사진"), {
      target: { files: [croppedFile] },
    });
    fireEvent.click(screen.getByRole("button", { name: "테스트 이미지 적용" }));

    await waitFor(() => expect(onAvatarImageChange).toHaveBeenCalledWith(
      "data:image/png;base64,YXZhdGFy"
    ));
    expect(screen.getByText("입장할 때 사진이 저장됩니다.")).toBeTruthy();
  });

  it("retries preflight without presenting editable join-profile fields", () => {
    const onJoin = vi.fn();

    render(
      <GuestJoinProfilePanel
        displayName="Guest"
        status="방 세션을 브라우저에 영구 저장할 수 없습니다."
        retryMode="preflight"
        onDisplayNameChange={vi.fn()}
        onAvatarImageChange={vi.fn()}
        onJoin={onJoin}
      />
    );

    expect(screen.getByRole("region", { name: "입장 확인 재시도" })).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: "이름" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    expect(onJoin).toHaveBeenCalledOnce();
  });

  it("retries a frozen join intent without presenting editable profile fields", () => {
    render(
      <GuestJoinProfilePanel
        displayName="Guest"
        status="입장 응답을 확인하지 못했습니다."
        retryMode="join"
        onDisplayNameChange={vi.fn()}
        onAvatarImageChange={vi.fn()}
        onJoin={vi.fn()}
      />
    );

    expect(screen.getByRole("region", { name: "입장 재시도" })).toBeTruthy();
    expect(screen.queryByRole("textbox", { name: "이름" })).toBeNull();
    expect(screen.getByRole("button", { name: "다시 시도" })).toBeTruthy();
  });

});
