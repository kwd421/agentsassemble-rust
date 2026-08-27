import { expect, test } from "@playwright/test";

const RECOVERY_CODE = "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567";

test("fails closed when a browser opens the product without server-owned authority", async ({
  page,
}) => {
  await page.goto("/");

  await expect(
    page.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "#general", exact: true })).toHaveCount(0);
});

test("does not admit legacy query or fragment bypass markers", async ({ page }) => {
  await page.goto("/join?guest=1#invite=legacy");

  await expect(
    page.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })
  ).toBeVisible();
});

test("retains the server-owned invite entrance", async ({ page }) => {
  await page.route("**/api/room-invite/admission", (route) =>
    route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        status: "profile_required",
        can_auto_join: false,
        room_id: "room-1",
        room_label: "Room One",
        invite_scope: "room",
      }),
    })
  );
  await page.goto("/join?token=invite-token");

  await expect(page.getByRole("region", { name: "입장 프로필" })).toBeVisible();
  await expect(
    page.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })
  ).toHaveCount(0);
});

test("does not let legacy query state override a server-owned invite", async ({ page }) => {
  await page.route("**/api/room-invite/admission", (route) => route.abort());
  await page.goto(
    "/join?token=invite-token&guest=1&room=legacy-room&scope=read_only"
  );

  await expect(page.getByRole("region", { name: "입장 확인 재시도" })).toBeVisible();
  await expect(page.getByRole("button", { name: "다시 시도", exact: true })).toBeVisible();
  await expect(page.getByText("legacy-room")).toHaveCount(0);
});

test("retains one frozen admission intent across response loss and a later invite gate", async ({
  page,
}) => {
  let preflightCount = 0;
  const joinBodies: unknown[] = [];
  await page.route("**/api/room-invite/admission", async (route) => {
    preflightCount += 1;
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        status: "known_user",
        can_auto_join: true,
        room_id: "room-1",
        room_label: "Room One",
        invite_scope: "room",
        participant: {
          participant_id: "guest-1",
          display_name: "Guest",
          avatar_image_url: "",
        },
        operator: false,
      }),
    });
  });
  await page.route("**/api/room-invite/join", async (route) => {
    joinBodies.push(route.request().postDataJSON());
    if (joinBodies.length === 2) {
      await route.fulfill({
        status: 403,
        contentType: "application/json",
        body: JSON.stringify({ code: "invite_revoked", error: "Invite was revoked." }),
      });
      return;
    }
    await route.abort("connectionrefused");
  });

  await page.goto("/join?token=invite-token");
  await expect(page.getByRole("region", { name: "입장 재시도" })).toBeVisible();
  await expect.poll(() => joinBodies.length).toBe(1);

  await page.reload();
  await expect.poll(() => joinBodies.length).toBe(2);

  await page.reload();
  await expect(page.getByRole("region", { name: "입장 재시도" })).toBeVisible();
  await expect.poll(() => joinBodies.length).toBe(3);

  expect(preflightCount).toBe(1);
  expect(joinBodies[1]).toEqual(joinBodies[0]);
  expect(joinBodies[2]).toEqual(joinBodies[0]);
});

test("retains pairing while consuming its URL secret", async ({ page }) => {
  await page.goto("/pair?token=aap1_pairing-token");

  await expect(page).toHaveURL(/\/pair$/);
  await expect(page.getByRole("region", { name: "운영자 기기 연결" })).toBeVisible();
});

test("keeps a one-use entrance when durable credential custody fails", async ({ page }) => {
  await page.addInitScript(() => {
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === "agentsassemble.browserCredential.v1") {
        throw new Error("storage unavailable");
      }
      setItem.call(this, key, value);
    };
  });

  await page.goto("/pair?token=aap1_pairing-token");

  await expect(page).toHaveURL(/\/pair\?token=aap1_pairing-token$/);
  await expect(page.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeVisible();
});

test("keeps a one-use entrance when durable client-id custody fails", async ({ page }) => {
  await page.addInitScript(() => {
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) {
      if (key === "agentsassemble.clientId.v1") {
        throw new Error("storage unavailable");
      }
      setItem.call(this, key, value);
    };
  });

  await page.goto("/join?token=invite-token");

  await expect(page).toHaveURL(/\/join\?token=invite-token$/);
  await expect(page.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeVisible();
});

test("retains recovery while consuming its URL secret", async ({ page }) => {
  await page.goto(`/?recover=1&room=friend-room#recovery=${RECOVERY_CODE}`);

  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("region", { name: "게스트 신원 복구" })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "복구 코드" })).toHaveValue(
    RECOVERY_CODE
  );
});
