// @vitest-environment-options {"url":"https://trusted-shell.test/"}
import "../test/nativeDialog";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fetchUserProfile, saveUserProfile, uploadUserProfileAvatar } from "./userProfile";
import { installRemoteWorkspace, closeRemoteWorkspace } from "../lib/remote/remoteWorkspace";
import { remoteResourcePath } from "../lib/remote/remoteResources";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import UserPanel from "../views/components/UserPanel";

const profile = { revision: 1, display_name: "Remote owner", handle: "remote", status: "online", custom_status: "",
  avatar_label: "RO", avatar_image_url: "/api/attachments/avatar_1234?view=1", banner_preset: "default",
  accent_color: "#5865f2", mic_muted: false, deafened: false, created_at: "", updated_at: "" };
const token = `aaos1.${"R".repeat(43)}`;
const identity = { centralOwner: true, centralSession: { sessionToken: token, generation: 7 }, deviceToken: "device" };
function workspace() {
  // JSDOM storage requires a non-opaque origin; expose only Tauri's document
  // location through the window boundary used by the production profile owner.
  vi.stubGlobal("window", new Proxy(window, { get: (target, key) => key === "location"
    ? { href: "tauri://localhost/", origin: "null" } : Reflect.get(target, key) }));
  const callbacks = new Set<() => void>();
  const transport = { hello: { origin: "https://host.test" }, active: true,
    fetch: vi.fn(async (path: string) => path.startsWith("/api/attachments/")
      ? new Response(new Uint8Array([137, 80, 78, 71]), { headers: { "content-type": "image/png" } })
      : new Response(JSON.stringify(path === "/api/attachments" ? { attachment: { url: profile.avatar_image_url } } : { profile }))),
    onClose: (fn: () => void) => { callbacks.add(fn); return () => callbacks.delete(fn); },
    close: () => { transport.active = false; for (const fn of callbacks) fn(); } };
  installRemoteWorkspace({ transport: transport as unknown as RemoteTransport, owner: { sessionToken: token } as never,
    deviceToken: "device", clientId: "client" });
  const invoke = vi.fn(); Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
  const plain = vi.fn(); vi.stubGlobal("fetch", plain);
  return { transport, invoke, plain };
}
afterEach(() => { cleanup(); closeRemoteWorkspace(); Reflect.deleteProperty(window, "__TAURI_INTERNALS__"); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("reads, saves and uploads against exact admitted host custody in a Tauri shell, without plaintext on close", async () => {
  const { transport, invoke, plain } = workspace();
  const snapshot = await fetchUserProfile(identity);
  expect(snapshot.displayResourceBase).toBe("https://host.test");
  expect((await saveUserProfile(snapshot.profile, 1, identity)).displayResourceBase).toBe("https://host.test");
  expect(await uploadUserProfileAvatar(new File(["image"], "avatar.png"), identity)).toBe(profile.avatar_image_url);
  for (const [, init] of transport.fetch.mock.calls as unknown as [string, RequestInit][]) {
    expect(new Headers(init.headers).get("authorization")).toBe(`Bearer ${token}`);
  }
  closeRemoteWorkspace();
  await expect(fetchUserProfile(identity)).rejects.toThrow("연결이 끝났어요");
  expect(invoke).not.toHaveBeenCalled(); expect(plain).not.toHaveBeenCalled();
});

it("loads profile, card and edit-preview avatars as encrypted Blob URLs and revokes them", async () => {
  const { transport, invoke, plain } = workspace();
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:encrypted-avatar");
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const view = render(<UserPanel onlineCount={1} agentCount={0} hasBackendError={false} profileIdentity={identity} />);
  const user = await screen.findByRole("button", { name: /Remote owner/ });
  await waitFor(() => expect(view.container.querySelector(".dc-user-panel")?.getAttribute("style")).toContain("blob:encrypted-avatar"));
  fireEvent.click(user);
  expect(view.container.querySelector(".dc-profile-banner")?.getAttribute("style")).toContain("blob:encrypted-avatar");
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  await waitFor(() => expect(document.querySelector(".dc-user-settings-preview-card")?.getAttribute("style")).toContain("blob:encrypted-avatar"));
  expect(transport.fetch.mock.calls.some(([path]) => path === profile.avatar_image_url)).toBe(true);
  expect(invoke).not.toHaveBeenCalled(); expect(plain).not.toHaveBeenCalled();
  closeRemoteWorkspace();
  expect(revoke).toHaveBeenCalledWith("blob:encrypted-avatar");
});

it("routes only matching host attachment/avatar resources into the channel", () => {
  workspace();
  expect(remoteResourcePath(profile.avatar_image_url)).toBe(profile.avatar_image_url);
  expect(remoteResourcePath(`https://host.test${profile.avatar_image_url}`)).toBe(profile.avatar_image_url);
  expect(remoteResourcePath(`https://foreign.test${profile.avatar_image_url}`)).toBeNull();
  expect(() => remoteResourcePath("https://host.test/api/user-profile")).toThrow("사진 주소");
});
