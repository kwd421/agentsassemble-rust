import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_USER_PROFILE } from "../../lib/userProfileModel";
import type { UserProfile, UserProfileSnapshot } from "../../api";
import UserPanel from "./UserPanel";

const apiMocks = vi.hoisted(() => ({
  fetchUserProfile: vi.fn(),
  saveUserProfile: vi.fn(),
  uploadUserProfileAvatar: vi.fn(),
}));

vi.mock("../../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../api")>();
  return {
    ...actual,
    fetchUserProfile: apiMocks.fetchUserProfile,
    saveUserProfile: apiMocks.saveUserProfile,
    uploadUserProfileAvatar: apiMocks.uploadUserProfileAvatar,
  };
});

vi.mock("./ImageCropper", () => ({
  default: ({ onCropped }: { onCropped: (file: File) => void }) => (
    <button
      type="button"
      onClick={() =>
        onCropped(new File(["avatar"], "avatar.png", { type: "image/png" }))
      }
    >
      적용
    </button>
  ),
}));

function snapshot(
  profile: UserProfile,
  displayResourceBase = "http://localhost:3000",
  revision = 1
): UserProfileSnapshot {
  return { profile, revision, displayResourceBase };
}

describe("UserPanel", () => {
  beforeEach(() => {
    apiMocks.fetchUserProfile.mockReset();
    apiMocks.saveUserProfile.mockReset();
    apiMocks.uploadUserProfileAvatar.mockReset();
  });

  it("does not present the local default as authority before server hydration", async () => {
    let resolveProfile: ((profile: UserProfileSnapshot) => void) | undefined;
    apiMocks.fetchUserProfile.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveProfile = resolve;
        })
    );

    render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ deviceToken: "device-token" }}
      />
    );

    expect(screen.queryByRole("button", { name: /SeiNel/ })).toBeNull();
    expect(screen.getByRole("status", { name: "프로필 불러오는 중" })).toBeTruthy();

    await waitFor(() => expect(apiMocks.fetchUserProfile).toHaveBeenCalledTimes(1));
    resolveProfile?.(
      snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Server Authority" })
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Server Authority/ })).toBeTruthy()
    );
  });

  it("lets an admitted guest edit the same authenticated profile shown in the room", async () => {
    const loaded = {
      ...DEFAULT_USER_PROFILE,
      displayName: "Guest Before",
      avatarLabel: "GB",
    };
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(loaded));
    apiMocks.saveUserProfile.mockImplementation(async (profile) => snapshot(profile));

    render(
      <UserPanel
        onlineCount={2}
        agentCount={1}
        hasBackendError={false}
        guestProfile={{
          displayName: "Guest Before",
          avatarLabel: "GB",
          statusLabel: "온라인",
        }}
        profileIdentity={{ sessionToken: "guest-session" }}
      />
    );

    await waitFor(() =>
      expect(apiMocks.fetchUserProfile).toHaveBeenCalledWith({
        sessionToken: "guest-session",
      })
    );
    fireEvent.click(screen.getByRole("button", { name: /Guest Before/ }));
    fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
    fireEvent.click(screen.getByRole("button", { name: /계정/ }));
    expect(screen.queryByLabelText("공개 계정 연결")).toBeNull();
    fireEvent.change(screen.getByLabelText("표시 이름"), {
      target: { value: "Guest After" },
    });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));

    await waitFor(() =>
      expect(apiMocks.saveUserProfile).toHaveBeenCalledWith(
        expect.objectContaining({ displayName: "Guest After" }),
        1,
        { sessionToken: "guest-session" }
      )
    );
    expect(screen.getByRole("button", { name: /Guest After/ })).toBeTruthy();
  });

  it("waits for guest admission before reading the server-owned profile", async () => {
    const loaded = {
      ...DEFAULT_USER_PROFILE,
      displayName: "Guest Joined",
      avatarLabel: "GJ",
    };
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(loaded));

    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        guestProfile={{
          displayName: "Guest Pending",
          avatarLabel: "GP",
          statusLabel: "온라인",
        }}
        profileIdentity={{ deviceToken: "guest-device" }}
      />
    );

    expect(apiMocks.fetchUserProfile).not.toHaveBeenCalled();
    expect(within(view.container).getByText("Guest Pending")).toBeTruthy();

    view.rerender(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        guestProfile={{
          displayName: "Guest Joined",
          avatarLabel: "GJ",
          statusLabel: "온라인",
        }}
        profileIdentity={{
          sessionToken: "guest-session",
          deviceToken: "guest-device",
        }}
      />
    );

    await waitFor(() =>
      expect(apiMocks.fetchUserProfile).toHaveBeenLastCalledWith({
        sessionToken: "guest-session",
        deviceToken: "guest-device",
      })
    );
    expect(
      within(view.container).getByRole("button", { name: /Guest Joined/ })
    ).toBeTruthy();
  });

  it("uses one profile-photo editor instead of exposing the stored attachment URL", async () => {
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(DEFAULT_USER_PROFILE));

    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ deviceToken: "device-token" }}
      />
    );

    await waitFor(() => expect(apiMocks.fetchUserProfile).toHaveBeenCalled());
    fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정" }));
    fireEvent.click(within(view.container).getByRole("button", { name: /프로필/ }));

    expect(within(view.container).queryByLabelText("아바타 이미지 URL")).toBeNull();
    fireEvent.click(within(view.container).getByRole("button", { name: "프로필 사진 변경" }));
    expect(within(view.container).getByRole("dialog", { name: "프로필 사진 수정" })).toBeTruthy();
    expect(within(view.container).getByLabelText("이미지 선택")).toBeTruthy();
  });

  it("admits only one complete avatar upload and bind submission at a time", async () => {
    let resolveUpload: ((avatar: string) => void) | undefined;
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(DEFAULT_USER_PROFILE));
    apiMocks.uploadUserProfileAvatar.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveUpload = resolve;
        })
    );
    apiMocks.saveUserProfile.mockImplementation(async (profile) => snapshot(profile));
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ deviceToken: "device-token" }}
      />
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /SeiNel/ })).toBeTruthy()
    );
    fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정" }));
    fireEvent.click(within(view.container).getByRole("button", { name: /프로필/ }));
    fireEvent.click(
      within(view.container).getByRole("button", { name: "프로필 사진 변경" })
    );
    fireEvent.change(within(view.container).getByLabelText("이미지 선택"), {
      target: {
        files: [new File(["source"], "source.png", { type: "image/png" })],
      },
    });
    const apply = await within(view.container).findByRole("button", { name: "적용" });
    fireEvent.click(apply);
    fireEvent.click(apply);
    await waitFor(() => expect(apiMocks.uploadUserProfileAvatar).toHaveBeenCalledTimes(1));

    resolveUpload?.("/api/attachments/avatar?view=1");
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1));
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ avatarImage: "/api/attachments/avatar?view=1" }),
      1,
      { deviceToken: "device-token" }
    );
  });

  it("does not bind an uploaded avatar after the profile identity changes", async () => {
    let resolveUpload: ((avatar: string) => void) | undefined;
    apiMocks.fetchUserProfile
      .mockResolvedValueOnce(snapshot(DEFAULT_USER_PROFILE))
      .mockResolvedValueOnce(
        snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Replacement Identity" })
      );
    apiMocks.uploadUserProfileAvatar.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveUpload = resolve;
        })
    );
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ sessionToken: "session-a" }}
      />
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /SeiNel/ })).toBeTruthy()
    );
    fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정" }));
    fireEvent.click(within(view.container).getByRole("button", { name: /프로필/ }));
    fireEvent.click(
      within(view.container).getByRole("button", { name: "프로필 사진 변경" })
    );
    fireEvent.change(within(view.container).getByLabelText("이미지 선택"), {
      target: {
        files: [new File(["source"], "source.png", { type: "image/png" })],
      },
    });
    fireEvent.click(await within(view.container).findByRole("button", { name: "적용" }));
    await waitFor(() => expect(apiMocks.uploadUserProfileAvatar).toHaveBeenCalledTimes(1));

    view.rerender(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ sessionToken: "session-b" }}
      />
    );
    expect(apiMocks.fetchUserProfile).toHaveBeenCalledTimes(1);
    resolveUpload?.("/api/attachments/retired-avatar?view=1");

    await waitFor(() =>
      expect(apiMocks.fetchUserProfile).toHaveBeenLastCalledWith({
        sessionToken: "session-b",
      })
    );
    expect(apiMocks.saveUserProfile).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        within(view.container).getByRole("button", { name: /Replacement Identity/ })
      ).toBeTruthy()
    );
  });

  it("serializes profile mutations against the latest committed profile/base pair", async () => {
    const resolvers: Array<(value: UserProfileSnapshot) => void> = [];
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(DEFAULT_USER_PROFILE));
    apiMocks.saveUserProfile.mockImplementation(
      () => new Promise((resolve) => { resolvers.push(resolve); })
    );
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ deviceToken: "device-token" }}
      />
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /SeiNel/ })).toBeTruthy()
    );
    fireEvent.click(
      within(view.container).getByRole("button", { name: "마이크 음소거 해제" })
    );
    fireEvent.click(
      within(view.container).getByRole("button", { name: "헤드셋 끄기" })
    );
    await waitFor(() => expect(resolvers).toHaveLength(1));
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1);
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ micMuted: false, deafened: false }),
      1,
      { deviceToken: "device-token" }
    );

    resolvers[0](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "First Authority",
          micMuted: false,
          avatarImage: "/api/attachments/first_avatar?view=1",
        },
        "http://127.0.0.1:49171",
        2
      )
    );
    await waitFor(() => expect(resolvers).toHaveLength(2));
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(2);
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ micMuted: false, deafened: true }),
      2,
      { deviceToken: "device-token" }
    );

    resolvers[1](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "New Authority",
          micMuted: false,
          deafened: true,
          avatarImage: "/api/attachments/new_avatar?view=1",
        },
        "http://127.0.0.1:49172",
        3
      )
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /New Authority/ })).toBeTruthy()
    );

    expect(within(view.container).queryByText("First Authority")).toBeNull();
    expect(
      (view.container.querySelector(".dc-user-panel") as HTMLElement).style.getPropertyValue(
        "--profile-avatar-image"
      )
    ).toContain("http://127.0.0.1:49172/api/attachments/new_avatar?view=1");
  });

  it("waits for a held server-wide save before hydrating a replacement identity", async () => {
    let resolveSave: ((value: UserProfileSnapshot) => void) | undefined;
    apiMocks.fetchUserProfile
      .mockResolvedValueOnce(snapshot(DEFAULT_USER_PROFILE))
      .mockResolvedValueOnce(
        snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Replacement Identity" })
      );
    apiMocks.saveUserProfile.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        })
    );
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ sessionToken: "session-a" }}
      />
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /SeiNel/ })).toBeTruthy()
    );
    fireEvent.click(
      within(view.container).getByRole("button", { name: "마이크 음소거 해제" })
    );
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1));

    view.rerender(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ sessionToken: "session-b" }}
      />
    );
    expect(apiMocks.fetchUserProfile).toHaveBeenCalledTimes(1);

    resolveSave?.(
      snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Retired Identity Save" })
    );
    await waitFor(() =>
      expect(apiMocks.fetchUserProfile).toHaveBeenLastCalledWith({
        sessionToken: "session-b",
      })
    );
    await waitFor(() =>
      expect(
        within(view.container).getByRole("button", { name: /Replacement Identity/ })
      ).toBeTruthy()
    );
    expect(within(view.container).queryByText("Retired Identity Save")).toBeNull();
  });

  it("rehydrates and cancels queued intents after an unknown save outcome", async () => {
    let rejectSave: ((error: Error) => void) | undefined;
    apiMocks.fetchUserProfile
      .mockResolvedValueOnce(snapshot(DEFAULT_USER_PROFILE))
      .mockResolvedValueOnce(
        snapshot(
          { ...DEFAULT_USER_PROFILE, displayName: "Recovered Authority" },
          "http://localhost:3000",
          2
        )
      );
    apiMocks.saveUserProfile
      .mockImplementationOnce(
        () =>
          new Promise((_, reject) => {
            rejectSave = reject;
          })
      )
      .mockImplementation(async (profile) => snapshot(profile));
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ sessionToken: "guest-session" }}
      />
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /SeiNel/ })).toBeTruthy()
    );
    fireEvent.click(
      within(view.container).getByRole("button", { name: "마이크 음소거 해제" })
    );
    fireEvent.click(
      within(view.container).getByRole("button", { name: "헤드셋 끄기" })
    );
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1));

    rejectSave?.(new Error("profile response lost"));
    await waitFor(() =>
      expect(
        within(view.container).getByRole("button", { name: /Recovered Authority/ })
      ).toBeTruthy()
    );
    expect(apiMocks.fetchUserProfile).toHaveBeenCalledTimes(2);
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1);

    fireEvent.click(
      within(view.container).getByRole("button", { name: "헤드셋 끄기" })
    );
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(2));
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ displayName: "Recovered Authority", deafened: true }),
      2,
      { sessionToken: "guest-session" }
    );
  });
});
