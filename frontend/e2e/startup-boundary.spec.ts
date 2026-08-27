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
  await page.goto("/join?token=invite-token");

  await expect(page.getByRole("region", { name: "입장 프로필" })).toBeVisible();
  await expect(
    page.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })
  ).toHaveCount(0);
});

test("does not let legacy query state override a server-owned invite", async ({ page }) => {
  await page.goto(
    "/join?token=invite-token&guest=1&room=legacy-room&scope=read_only"
  );

  await expect(page.getByRole("region", { name: "입장 프로필" })).toBeVisible();
  await expect(page.getByRole("button", { name: "초대 확인 중", exact: true })).toBeVisible();
  await expect(page.getByText("legacy-room")).toHaveCount(0);
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

test("retains recovery while consuming its URL secret", async ({ page }) => {
  await page.goto(`/?recover=1&room=friend-room#recovery=${RECOVERY_CODE}`);

  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("region", { name: "게스트 신원 복구" })).toBeVisible();
  await expect(page.getByRole("textbox", { name: "복구 코드" })).toHaveValue(
    RECOVERY_CODE
  );
});
