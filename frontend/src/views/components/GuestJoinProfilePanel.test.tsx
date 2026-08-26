import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

const apiMocks = vi.hoisted(() => ({
  uploadLobbyAttachment: vi.fn(),
}));
const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(
    () => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  ),
}));

vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: deviceMocks.getOrCreateBrowserCredential,
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
    deviceMocks.getOrCreateBrowserCredential.mockReset();
    deviceMocks.getOrCreateBrowserCredential.mockReturnValue(
      "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
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

  it("stops a pre-join upload before network I/O without durable browser custody", async () => {
    deviceMocks.getOrCreateBrowserCredential.mockImplementation(() => {
      throw new Error("브라우저 저장소를 사용할 수 없습니다.");
    });
    const croppedFile = new File(["avatar"], "avatar.png", { type: "image/png" });

    render(
      <GuestJoinProfilePanel
        inviteToken="aaj1_valid-invite"
        displayName="Guest"
        onDisplayNameChange={vi.fn()}
        onAvatarImageChange={vi.fn()}
        onJoin={vi.fn()}
      />
    );

    fireEvent.change(screen.getByLabelText("프로필 사진"), {
      target: { files: [croppedFile] },
    });
    fireEvent.click(screen.getByRole("button", { name: "테스트 이미지 적용" }));

    await waitFor(() => expect(screen.getByText(/브라우저 저장소/)).toBeTruthy());
    expect(apiMocks.uploadLobbyAttachment).not.toHaveBeenCalled();
  });
});
