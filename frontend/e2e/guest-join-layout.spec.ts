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

    for (const name of ["게스트로 참가", "로그인하고 참가"]) {
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
    expect(nameField!.y).toBeGreaterThanOrEqual(row!.y);
    expect(nameField!.y + nameField!.height).toBeLessThanOrEqual(row!.y + row!.height);
    expect(nameField!.x + nameField!.width).toBeCloseTo(row!.x + row!.width, 0);
    await expect(avatarRow.getByLabel("프로필 사진")).toHaveAttribute("type", "file");
    await expect(avatarRow.getByText("프로필 사진", { exact: true })).toHaveCount(0);
    await expect(page.locator(".dc-join-identity-icon")).toHaveCSS("width", "64px");
    expect(await page.locator(".dc-guest-join-panel").evaluate(element =>
      getComputedStyle(element).backgroundColor)).toMatch(/^rgb\(/);
    await page.screenshot({ path: testInfo.outputPath("join-profile.png") });

    if (admission === "anonymous") {
      const request = page.waitForRequest("**/api/room-invite/join");
      await page.getByRole("button", { name: "게스트로 참가", exact: true }).click();
      expect((await request).postDataJSON()).toMatchObject({ display_name: "Layout Guest" });
    } else {
      await page.getByRole("button", { name: "로그인하고 참가", exact: true }).click();
      await expect(page.getByRole("region", { name: "서버 참가", exact: true })).toBeVisible();
    }
  });
}
