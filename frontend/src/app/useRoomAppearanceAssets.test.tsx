import { act, renderHook, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RoomAppearance } from "../lib/roomAppearance";
import type { RoomDockItem } from "../lib/roomDockModel";

const api = vi.hoisted(() => ({ fetchBlob: vi.fn(), upload: vi.fn() }));
vi.mock("../api/roomAppearance", async () => ({
  ...(await vi.importActual<typeof import("../api/roomAppearance")>(
    "../api/roomAppearance"
  )),
  fetchRoomAppearanceBlob: api.fetchBlob,
  uploadRoomAppearance: api.upload,
}));

import { useRoomAppearanceAssets } from "./useRoomAppearanceAssets";

const bindUploadedReference = vi.fn();

const manager = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002",
  room_id: "general",
  room_uid: "30000000-0000-4000-8000-000000000003",
};
const room: RoomDockItem = {
  id: "general",
  label: "General",
  meetingId: "general",
  roomUid: manager.room_uid,
  serverId: manager.server_id,
  roomOrigin: "local",
  connectionState: "local",
  topic: "",
  shortLabel: "G",
  icon: Hash,
  createdAt: "",
  tone: "resident",
};
const banner = `/api/attachments/ra_${"a".repeat(32)}?view=1`;
const icon = `/api/attachments/ra_${"b".repeat(32)}?view=1`;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function renderAssets(
  appearance: RoomAppearance,
  status: "saving" | "ready" = "ready",
  rooms = [room]
) {
  return renderHook(
    ({
      currentAppearance,
      currentStatus,
      currentRooms,
      currentLocalAuthority,
    }) =>
      useRoomAppearanceAssets({
        rooms: currentRooms,
        activeRoomId: room.id,
        activeRemoteRoomId: "",
        remoteSessionToken: "",
        canonicalAppearanceFor: () => currentAppearance,
        settingsStateFor: () => ({ status: currentStatus }),
        localAuthorityCurrent: currentLocalAuthority,
        resolveLocalManager: () => manager,
        bindUploadedReference,
      }),
    {
      initialProps: {
        currentAppearance: appearance,
        currentStatus: status,
        currentRooms: rooms,
        currentLocalAuthority: true,
      },
    }
  );
}

describe("room appearance object URL lifecycle", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    bindUploadedReference.mockResolvedValue(undefined);
    let sequence = 0;
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL: vi.fn(() => `blob:appearance-${++sequence}`),
      revokeObjectURL: vi.fn(),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("replaces a pending preview with a bound read and revokes it after render", async () => {
    api.fetchBlob.mockResolvedValue(new Blob(["png"], { type: "image/png" }));
    const appearance: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderAssets(appearance, "saving");

    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room).bannerImage).toBe(
        "blob:appearance-1"
      )
    );
    expect(api.fetchBlob).toHaveBeenNthCalledWith(
      1,
      banner,
      { kind: "local", manager },
      "pending",
      expect.any(AbortSignal)
    );

    hook.rerender({
      currentAppearance: appearance,
      currentStatus: "ready",
      currentRooms: [room],
      currentLocalAuthority: true,
    });
    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room).bannerImage).toBe(
        "blob:appearance-2"
      )
    );
    expect(api.fetchBlob).toHaveBeenNthCalledWith(
      2,
      banner,
      { kind: "local", manager },
      "bound",
      expect.any(AbortSignal)
    );
    await waitFor(() =>
      expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:appearance-1")
    );
  });

  it("deduplicates one canonical asset shared by banner and icon", async () => {
    api.fetchBlob.mockResolvedValue(new Blob(["png"], { type: "image/png" }));
    const appearance: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      iconImage: banner,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderAssets(appearance);

    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room)).toMatchObject({
        bannerImage: "blob:appearance-1",
        iconImage: "blob:appearance-1",
      })
    );
    expect(api.fetchBlob).toHaveBeenCalledOnce();
    expect(URL.createObjectURL).toHaveBeenCalledOnce();
  });

  it("loads inactive room icons without retaining their unused banners", async () => {
    api.fetchBlob.mockResolvedValue(new Blob(["png"], { type: "image/png" }));
    const inactive = {
      ...room,
      id: "inactive",
      meetingId: "inactive",
      roomUid: "40000000-0000-4000-8000-000000000004",
    };
    const inactiveAppearance: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      iconImage: icon,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderHook(() =>
      useRoomAppearanceAssets({
        rooms: [inactive],
        activeRoomId: room.id,
        activeRemoteRoomId: "",
        remoteSessionToken: "",
        canonicalAppearanceFor: () => inactiveAppearance,
        settingsStateFor: () => ({ status: "ready" }),
        localAuthorityCurrent: true,
        resolveLocalManager: () => ({ ...manager, room_id: "inactive", room_uid: inactive.roomUid! }),
        bindUploadedReference,
      })
    );

    await waitFor(() =>
      expect(hook.result.current.appearanceFor(inactive).iconImage).toBe(
        "blob:appearance-1"
      )
    );
    expect(hook.result.current.appearanceFor(inactive).bannerImage).toBeUndefined();
    expect(api.fetchBlob).toHaveBeenCalledOnce();
    expect(api.fetchBlob).toHaveBeenCalledWith(
      icon,
      expect.anything(),
      "bound",
      expect.any(AbortSignal)
    );
  });

  it("aborts superseded reads and revokes every installed URL on removal", async () => {
    const pending = deferred<Blob>();
    api.fetchBlob.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(
      new Blob(["new"], { type: "image/png" })
    );
    const first: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderAssets(first);
    const firstSignal = api.fetchBlob.mock.calls[0]?.[3] as AbortSignal;

    hook.rerender({
      currentAppearance: { ...first, bannerImage: icon },
      currentStatus: "ready",
      currentRooms: [room],
      currentLocalAuthority: true,
    });
    expect(firstSignal.aborted).toBe(true);
    await act(async () => pending.resolve(new Blob(["old"], { type: "image/png" })));
    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room).bannerImage).toBe(
        "blob:appearance-1"
      )
    );
    expect(URL.createObjectURL).toHaveBeenCalledOnce();

    hook.rerender({
      currentAppearance: { ...first, bannerImage: undefined },
      currentStatus: "ready",
      currentRooms: [room],
      currentLocalAuthority: true,
    });
    await waitFor(() =>
      expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:appearance-1")
    );
    expect(hook.result.current.appearanceFor(room).bannerImage).toBeUndefined();
  });

  it("fails closed, reports the error, and retries only on explicit request", async () => {
    api.fetchBlob
      .mockRejectedValueOnce(new Error("bound read rejected"))
      .mockResolvedValueOnce(new Blob(["png"], { type: "image/png" }));
    const appearance: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderAssets(appearance);

    await waitFor(() =>
      expect(hook.result.current.errorFor(room)).toBe("bound read rejected")
    );
    expect(hook.result.current.appearanceFor(room).bannerImage).toBeUndefined();
    expect(api.fetchBlob).toHaveBeenCalledOnce();

    act(() => hook.result.current.retry(room));
    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room).bannerImage).toBe(
        "blob:appearance-1"
      )
    );
    expect(api.fetchBlob).toHaveBeenCalledTimes(2);
  });

  it("revokes installed URLs as soon as local directory authority is unconfirmed", async () => {
    api.fetchBlob.mockResolvedValue(new Blob(["png"], { type: "image/png" }));
    const appearance: RoomAppearance = {
      bannerPreset: "custom",
      bannerImage: banner,
      notifications: "mentions",
      inviteScope: "room",
    };
    const hook = renderAssets(appearance);

    await waitFor(() =>
      expect(hook.result.current.appearanceFor(room).bannerImage).toBe(
        "blob:appearance-1"
      )
    );
    hook.rerender({
      currentAppearance: appearance,
      currentStatus: "ready",
      currentRooms: [room],
      currentLocalAuthority: false,
    });

    await waitFor(() =>
      expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:appearance-1")
    );
    expect(hook.result.current.appearanceFor(room).bannerImage).toBeUndefined();
    expect(hook.result.current.errorFor(room)).toContain("관리자 권위");
    expect(api.fetchBlob).toHaveBeenCalledOnce();
  });

  it("resolves the current manager before every upload", async () => {
    api.upload.mockResolvedValue({ reference: { assetId: "asset", url: banner } });
    const hook = renderAssets({
      bannerPreset: "default",
      notifications: "mentions",
      inviteScope: "room",
    });
    const file = new File(["png"], "banner.png", { type: "image/png" });

    await expect(hook.result.current.upload(room, file, "banner")).resolves.toBe(true);

    expect(api.upload).toHaveBeenCalledWith(file, manager);
    expect(bindUploadedReference).toHaveBeenCalledWith(room, "banner", banner);
  });

  it("lets only the latest upload for one room slot bind and publish completion", async () => {
    const first = deferred<{ reference: { assetId: string; url: string } }>();
    const second = deferred<{ reference: { assetId: string; url: string } }>();
    api.upload.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const hook = renderAssets({
      bannerPreset: "default",
      notifications: "mentions",
      inviteScope: "room",
    });
    const firstFile = new File(["first"], "first.png", { type: "image/png" });
    const secondFile = new File(["second"], "second.png", { type: "image/png" });

    const firstUpload = hook.result.current.upload(room, firstFile, "banner");
    const secondUpload = hook.result.current.upload(room, secondFile, "banner");
    second.resolve({ reference: { assetId: "second", url: icon } });
    await expect(secondUpload).resolves.toBe(true);
    first.resolve({ reference: { assetId: "first", url: banner } });
    await expect(firstUpload).resolves.toBe(false);

    expect(bindUploadedReference).toHaveBeenCalledOnce();
    expect(bindUploadedReference).toHaveBeenCalledWith(room, "banner", icon);
  });

  it("does not bind an upload after its appearance owner unmounts", async () => {
    const pending = deferred<{ reference: { assetId: string; url: string } }>();
    api.upload.mockReturnValueOnce(pending.promise);
    const hook = renderAssets({
      bannerPreset: "default",
      notifications: "mentions",
      inviteScope: "room",
    });

    const upload = hook.result.current.upload(
      room,
      new File(["png"], "banner.png", { type: "image/png" }),
      "banner"
    );
    hook.unmount();
    pending.resolve({ reference: { assetId: "pending", url: banner } });

    await expect(upload).resolves.toBe(false);
    expect(bindUploadedReference).not.toHaveBeenCalled();
  });
});
