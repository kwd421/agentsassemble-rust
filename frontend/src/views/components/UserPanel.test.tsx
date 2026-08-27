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

function snapshot(
  profile: UserProfile,
  displayResourceBase = "http://localhost:3000"
): UserProfileSnapshot {
  return { profile, displayResourceBase };
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
        profileIdentity={{ roomId: "general" }}
      />
    );

    expect(screen.queryByRole("button", { name: /SeiNel/ })).toBeNull();
    expect(screen.getByRole("status", { name: "프로필 불러오는 중" })).toBeTruthy();

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
    fireEvent.change(screen.getByLabelText("표시 이름"), {
      target: { value: "Guest After" },
    });
    fireEvent.click(screen.getByRole("button", { name: "저장" }));

    await waitFor(() =>
      expect(apiMocks.saveUserProfile).toHaveBeenCalledWith(
        expect.objectContaining({ displayName: "Guest After" }),
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

  it("does not let an older profile response replace the newer profile/base pair", async () => {
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
        profileIdentity={{ roomId: "general" }}
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
    await waitFor(() => expect(resolvers).toHaveLength(2));

    resolvers[1](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "New Authority",
          avatarImage: "/api/attachments/new_avatar?view=1",
        },
        "http://127.0.0.1:49172"
      )
    );
    await waitFor(() =>
      expect(within(view.container).getByRole("button", { name: /New Authority/ })).toBeTruthy()
    );
    resolvers[0](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "Old Authority",
          avatarImage: "/api/attachments/old_avatar?view=1",
        },
        "http://127.0.0.1:49171"
      )
    );
    await Promise.resolve();

    expect(within(view.container).queryByText("Old Authority")).toBeNull();
    expect(
      (view.container.querySelector(".dc-user-panel") as HTMLElement).style.getPropertyValue(
        "--profile-avatar-image"
      )
    ).toContain("http://127.0.0.1:49172/api/attachments/new_avatar?view=1");
  });
});
