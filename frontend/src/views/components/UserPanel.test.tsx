import "../../test/nativeDialog";
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

// A quick profile mutation: pick a status from the profile card.
function setStatus(container: HTMLElement, status: string) {
  if (!container.querySelector(".dc-profile-status-options")) {
    if (!container.querySelector(".dc-profile-menu-card")) {
      fireEvent.click(container.querySelector(".dc-user-identity") as HTMLElement);
    }
    fireEvent.click(container.querySelector(".dc-profile-menu-card button[aria-expanded]") as HTMLElement);
  }
  fireEvent.click(container.querySelector(`.dc-profile-status-option[data-status="${status}"]`) as HTMLElement);
}

describe("UserPanel", () => {
  beforeEach(() => {
    apiMocks.fetchUserProfile.mockReset();
    apiMocks.saveUserProfile.mockReset();
    apiMocks.uploadUserProfileAvatar.mockReset();
  });

  it("does not present the local default as authority before server hydration", async () => {    let resolveProfile: ((profile: UserProfileSnapshot) => void) | undefined;
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

  it("keeps paired room presentation separate from durable account authority", async () => {
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot(DEFAULT_USER_PROFILE));
    const view = render(
      <UserPanel onlineCount={1} agentCount={0} hasBackendError={false}
        pairedRoomSession
        guestProfile={{ displayName: "Room Host", avatarLabel: "RH", statusLabel: "운영자로 접속" }}
        profileIdentity={{ sessionToken: "paired-session", deviceToken: "paired-device" }} />
    );
    expect(within(view.container).getByLabelText("운영자 방 접속 프로필")).toBeTruthy();
    expect(within(view.container).getByTitle("계정과 프로필은 서버 컴퓨터의 앱에서 변경해요.")).toBeTruthy();
    expect(within(view.container).queryByRole("button", { name: "사용자 설정" })).toBeNull();
    expect(apiMocks.fetchUserProfile).not.toHaveBeenCalled();
    view.rerender(
      <UserPanel onlineCount={1} agentCount={0} hasBackendError={false}
        guestProfile={{ displayName: "Human", avatarLabel: "H", statusLabel: "게스트로 접속" }}
        profileIdentity={{ sessionToken: "human-session", deviceToken: "human-device" }} />
    );
    await waitFor(() => expect(apiMocks.fetchUserProfile).toHaveBeenCalledWith({
      sessionToken: "human-session", deviceToken: "human-device",
    }));
    const sharedProps = {
      onlineCount: 1, agentCount: 0, hasBackendError: false,
      profileIdentity: { sessionToken: "human-session", deviceToken: "human-device" },
    };
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Other device" }, "", 2));
    view.rerender(<UserPanel {...sharedProps} publishedProfileRevision={2} />);
    await waitFor(() => expect(within(view.container).getByRole("button", { name: /Other device/ })).toBeTruthy());
    fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정" }));
    fireEvent.change(within(view.container).getByLabelText("표시 이름"), { target: { value: "Unsent draft" } });
    apiMocks.fetchUserProfile.mockResolvedValue(snapshot({ ...DEFAULT_USER_PROFILE, displayName: "Newer device" }, "", 3));
    view.rerender(<UserPanel {...sharedProps} publishedProfileRevision={3} />);
    expect((within(view.container).getByLabelText("표시 이름") as HTMLInputElement).value).toBe("Unsent draft");
    fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정 닫기" }));
    await waitFor(() => expect(within(view.container).getByRole("button", { name: /Newer device/ })).toBeTruthy());
  });

  it.each([false, true])("waits for admission before reading the server-owned profile (central owner: %s)", async (centralOwner) => {
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
        profileAuthorityReady={!centralOwner}
        guestProfile={centralOwner ? undefined : {
          displayName: "Guest Pending",
          avatarLabel: "GP",
          statusLabel: "온라인",
        }}
        profileIdentity={{ deviceToken: "guest-device" }}
      />
    );

    expect(apiMocks.fetchUserProfile).not.toHaveBeenCalled();
    expect(within(view.container).getByText(centralOwner ? "프로필 불러오는 중" : "Guest Pending")).toBeTruthy();

    view.rerender(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileAuthorityReady
        guestProfile={centralOwner ? undefined : {
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
    fireEvent.change(within(view.container).getByLabelText("이미지 선택"), {
      target: { files: [new File(["source"], "source.png", { type: "image/png" })] },
    });
    expect(within(view.container).getByRole("dialog", { name: "프로필 사진 수정" })).toBeTruthy();
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
    setStatus(view.container, "idle");
    setStatus(view.container, "dnd");
    await waitFor(() => expect(resolvers).toHaveLength(1));
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1);
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ status: "idle" }),
      1,
      { deviceToken: "device-token" }
    );

    resolvers[0](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "First Authority",
          status: "idle",
          avatarImage: "/api/attachments/first_avatar?view=1",
        },
        "http://127.0.0.1:49171",
        2
      )
    );
    await waitFor(() => expect(resolvers).toHaveLength(2));
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(2);
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ displayName: "First Authority", status: "dnd" }),
      2,
      { deviceToken: "device-token" }
    );

    resolvers[1](
      snapshot(
        {
          ...DEFAULT_USER_PROFILE,
          displayName: "New Authority",
          status: "dnd",
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
    setStatus(view.container, "idle");
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
    setStatus(view.container, "idle");
    setStatus(view.container, "dnd");
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1));

    rejectSave?.(new Error("profile response lost"));
    await waitFor(() =>
      expect(
        within(view.container).getByRole("button", { name: /Recovered Authority/ })
      ).toBeTruthy()
    );
    expect(apiMocks.fetchUserProfile).toHaveBeenCalledTimes(2);
    expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(1);

    setStatus(view.container, "dnd");
    await waitFor(() => expect(apiMocks.saveUserProfile).toHaveBeenCalledTimes(2));
    expect(apiMocks.saveUserProfile).toHaveBeenLastCalledWith(
      expect.objectContaining({ displayName: "Recovered Authority", status: "dnd" }),
      2,
      { sessionToken: "guest-session" }
    );
  });

  it("stays neutral without internal text while the startup profile fails", async () => {
    apiMocks.fetchUserProfile.mockRejectedValue(
      new Error("데스크톱 호스트 제품 표면이 아직 고정되지 않았습니다.")
    );
    const view = render(
      <UserPanel
        onlineCount={1}
        agentCount={0}
        hasBackendError={false}
        profileIdentity={{ deviceToken: "device-token" }}
      />
    );
    await waitFor(() => expect(apiMocks.fetchUserProfile).toHaveBeenCalled());
    // Neutral Korean loading/failure copy only; internal jargon never
    // appears in the visible status (raw detail stays collapsed).
    const status = within(view.container).getByRole("status");
    expect(status.textContent).toContain("프로필을 불러오지 못했어요");
    expect(status.textContent).not.toContain("제품 표면");
    expect(status.textContent).not.toContain("고정");
    expect(within(view.container).getByText("자세히")).toBeTruthy();
  });

  it("keeps signed-in central settings reachable without local server authority", async () => {
    vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
    window.localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
      token: "token-b", expires_at: 9_999_999_999, device_id: "device",
      person: { person_id: "person-b", identity_kind: "google", display_name: "B계정" },
    }));
    try {
      const view = render(
        <UserPanel
          onlineCount={1}
          agentCount={0}
          hasBackendError={false}
          profileAuthorityReady={false}
          centralAccount={{ displayName: "B계정", avatarLabel: "B", statusLabel: "온라인" }}
          profileIdentity={{ deviceToken: "device-token" }}
        />
      );
      // No local profile fetch: the foreign host must never represent B.
      expect(apiMocks.fetchUserProfile).not.toHaveBeenCalled();
      expect(within(view.container).getByRole("button", { name: /B계정 계정 설정 열기/ })).toBeTruthy();
      fireEvent.click(within(view.container).getByRole("button", { name: "사용자 설정" }));
      expect(await within(view.container).findByRole("button", { name: "로그아웃" })).toBeTruthy();
      expect(within(view.container).getByText("이 컴퓨터의 서버 프로필은 서버를 등록한 뒤 바꿀 수 있어요. 계정 로그아웃은 아래에서 할 수 있어요.")).toBeTruthy();
    } finally {
      vi.unstubAllEnvs();
      window.localStorage.clear();
    }
  });
});
