import { fetchDesktopOperatorRuntime, initializeDesktopBootstrap, requestDesktopBootstrapStatus } from "./desktopBridge";
import type { CentralPerson } from "./central/identity";
import { rememberGuestProfile } from "./deviceIdentity";
import { responseError } from "../api/http";

export async function saveLocalProfile(
  displayName: string,
  bootstrapRequestId: string,
  googlePerson?: CentralPerson
) {
  const name = displayName.trim();
  if (!name) throw new Error("로컬 표시 이름이 비어 있습니다.");
  const current = await requestDesktopBootstrapStatus();
  let bootstrap =
    current.phase === "empty"
      ? await initializeDesktopBootstrap(bootstrapRequestId, name)
      : current;
  if (bootstrap.phase !== "complete" || !bootstrap.profile) {
    throw new Error("로컬 신원 권위를 안전하게 초기화하지 못했습니다.");
  }
  if (googlePerson?.identity_kind === "google" && googlePerson.avatar_url !== undefined && bootstrap.profile.revision === 1) {
    const imported = await fetchDesktopOperatorRuntime("/api/user-profile", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ expected_revision: 1, initial_google_profile: {
        display_name: googlePerson.display_name,
        picture_url: googlePerson.avatar_url || "",
      } }),
    });
    if (!imported.ok) throw await responseError(imported);
    bootstrap = await requestDesktopBootstrapStatus();
    if (bootstrap.phase !== "complete" || !bootstrap.profile) {
      throw new Error("Google 프로필의 저장된 로컬 권위를 확인하지 못했습니다.");
    }
  }
  rememberGuestProfile({
    displayName: bootstrap.profile.display_name,
    avatarImage: bootstrap.profile.avatar_image_url,
  });
  return bootstrap;
}

