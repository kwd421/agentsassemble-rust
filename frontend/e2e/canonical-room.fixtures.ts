import { expect, type Page } from "@playwright/test";
import { Buffer } from "node:buffer";

export const PROFILE_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZfNwAAAAASUVORK5CYII=",
  "base64"
);

const HOST_TOKEN_STORAGE_KEY = "agentsassemble.hostToken.v1";
const E2E_HOST_TOKEN = "e2e-host-token";
export const GUEST_SESSION_STORAGE_KEY = "agentsassemble.roomGuestSession.v1";
const STARTUP_IDENTITY_STORAGE_KEY = "agentsassemble.startupIdentity.v1";
const DEVICE_TOKEN_STORAGE_KEY = "agentsassemble.deviceToken.v1";
const E2E_DEVICE_TOKEN = "e2e-device-token";

export const HOST_API_HEADERS = {
  "X-Host-Token": E2E_HOST_TOKEN,
  "X-Device-Token": E2E_DEVICE_TOKEN,
};

export async function installHostRuntime(page: Page) {
  await page.addInitScript(
    ([hostKey, hostToken, identityKey, deviceKey, deviceToken]) => {
      window.sessionStorage.setItem(hostKey, hostToken);
      window.localStorage.setItem(identityKey, "selected");
      window.localStorage.setItem(deviceKey, deviceToken);
    },
    [
      HOST_TOKEN_STORAGE_KEY,
      E2E_HOST_TOKEN,
      STARTUP_IDENTITY_STORAGE_KEY,
      DEVICE_TOKEN_STORAGE_KEY,
      E2E_DEVICE_TOKEN,
    ]
  );
}

export async function openHostInviteDialog(page: Page) {
  await installHostRuntime(page);
  await page.goto("/");
  await page.getByRole("button", { name: "#general", exact: true }).click();
  await page.getByRole("button", { name: "서버에 초대하기" }).first().click();
  return page.getByRole("dialog", { name: /초대 및 연결/ });
}

export function toLocalFixtureUrl(inviteUrl: string) {
  const remote = new URL(inviteUrl);
  return `http://127.0.0.1:8898${remote.pathname}${remote.search}`;
}

export async function createGuestInviteUrl(page: Page) {
  const inviteDialog = await openHostInviteDialog(page);
  await inviteDialog
    .getByRole("region", { name: "사람 초대" })
    .getByRole("button", { name: "생성" })
    .click();
  const guestInviteInput = inviteDialog.getByRole("textbox", { name: "사람 초대 링크" });
  await expect(guestInviteInput).toHaveValue(/^https?:\/\/[^/]+\/join\?token=/);
  return guestInviteInput.inputValue();
}

export async function readGuestSession(page: Page) {
  return page.evaluate(
    (key) => JSON.parse(window.localStorage.getItem(key) || "null"),
    GUEST_SESSION_STORAGE_KEY
  );
}

export async function joinGuest(page: Page, inviteUrl: string, displayName: string) {
  await page.goto(toLocalFixtureUrl(inviteUrl));
  const profile = page.getByRole("region", { name: "입장 프로필" });
  await expect(profile).toBeVisible();
  await profile.getByRole("textbox", { name: "이름" }).fill(displayName);
  await profile.getByRole("button", { name: "입장", exact: true }).click();
  await expect(profile).toHaveCount(0);
  await expect.poll(() => readGuestSession(page)).not.toBeNull();
  return readGuestSession(page);
}

export async function roomWithLabel(page: Page, label: string) {
  const response = await page.request.get("/api/rooms", { headers: HOST_API_HEADERS });
  expect(response.ok()).toBe(true);
  const payload = (await response.json()) as {
    rooms?: Array<{ room_id?: string; label?: string }>;
  };
  return (payload.rooms || []).find((room) => room.label === label) || null;
}

export async function openActiveServerSettings(page: Page) {
  const header = page.getByRole("button", { name: /서버 메뉴 열기$/ });
  const accessibleName = await header.getAttribute("aria-label");
  const roomLabel = String(accessibleName || "").replace(/ 서버 메뉴 열기$/, "");
  await page.getByRole("button", { name: roomLabel, exact: true }).click({
    button: "right",
  });
  await page.getByRole("menuitem", { name: "서버 설정", exact: true }).click();
}
