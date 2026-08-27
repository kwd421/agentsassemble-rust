import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

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
  beforeEach(() => {
    apiMocks.uploadLobbyAttachment.mockReset();
  });

  afterEach(cleanup);

  it("uses the current invite only for an explicit pre-join profile upload", async () => {
    const onAvatarImageChange = vi.fn();
    const croppedFile = new File(["avatar"], "avatar.png", { type: "image/png" });
    apiMocks.uploadLobbyAttachment.mockResolvedValue({
      id: "avatar-12345678",
      filename: "avatar.png",
      content_type: "image/png",
      size: 6,
      is_image: true,
      url: "/api/attachments/avatar-12345678?view=1",
      download_url: "/api/attachments/avatar-12345678?download=1",
    });

    render(
      <GuestJoinProfilePanel
        deviceToken="aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        inviteToken="aaj1_valid-invite"
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

    await waitFor(() =>
      expect(apiMocks.uploadLobbyAttachment).toHaveBeenCalledWith(croppedFile, {
        inviteToken: "aaj1_valid-invite",
        deviceToken: "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        purpose: "profile_avatar",
      })
    );
    expect(onAvatarImageChange).toHaveBeenCalledWith(
      "/api/attachments/avatar-12345678?view=1"
    );
  });

  it("retries preflight without presenting editable join-profile fields", () => {
    const onJoin = vi.fn();

    render(
      <GuestJoinProfilePanel
        deviceToken="aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        inviteToken="aaj1_valid-invite"
        displayName="Guest"
        status="방 세션을 브라우저에 영구 저장할 수 없습니다."
        preflightRetry
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

});
