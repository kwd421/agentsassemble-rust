import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1024, height: 768 } });

for (const admission of ["anonymous", "central"] as const) {
  test(`join profile keeps ${admission} admission clickable`, async ({ page }, testInfo) => {
    await page.route("**/api/room-invite/admission", (route) => route.fulfill({
      json: { status: "profile_required", can_auto_join: false,
        room_id: "room-1", room_label: "Room One", invite_scope: "room" },
    }));
    await page.route("**/api/room-invite/join", (route) => route.fulfill({
      status: 403, json: { detail: "화면 테스트 입장 거절" },
    }));
    await page.route("**/api/room-invite/member-challenge", (route) => route.fulfill({
      status: 403, json: { detail: "화면 테스트 중앙 입장 거절" },
    }));
    await page.goto("/join?token=layout-test-invite");
    await page.getByRole("textbox", { name: "이름" }).fill("Layout Guest");

    for (const name of ["입장", "중앙 계정으로 입장"]) {
      const button = page.getByRole("button", { name, exact: true });
      await expect(button).toBeEnabled();
      expect(await button.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        return element.contains(document.elementFromPoint(
          rect.x + rect.width / 2, rect.y + rect.height / 2,
        ));
      })).toBe(true);
    }
    const avatarRow = page.locator(".dc-guest-avatar-row");
    await expect(avatarRow).toHaveCSS("position", "static");
    await expect(page.locator(".dc-guest-join-panel")).toHaveCSS("position", "fixed");
    const row = await avatarRow.boundingBox();
    const card = await page.locator(".dc-guest-join-card").boundingBox();
    const nameField = await page.getByRole("textbox", { name: "이름" }).boundingBox();
    expect(row!.x).toBeGreaterThan(card!.x);
    expect(row!.x + row!.width).toBeLessThan(card!.x + card!.width);
    expect(row!.y + row!.height).toBeLessThan(nameField!.y);
    await expect(avatarRow.getByText("프로필 사진")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("join-profile.png") });

    if (admission === "anonymous") {
      const request = page.waitForRequest("**/api/room-invite/join");
      await page.getByRole("button", { name: "입장", exact: true }).click();
      expect((await request).postDataJSON()).toMatchObject({ display_name: "Layout Guest" });
    } else {
      await page.getByRole("button", { name: "중앙 계정으로 입장", exact: true }).click();
      await expect(page.getByRole("region", { name: "중앙 계정 입장", exact: true })).toBeVisible();
    }
  });
}
