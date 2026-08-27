import { expect, test } from "@playwright/test";
import {
  lengthDelimitedTranscript,
  sha256Hex,
} from "../src/lib/lengthDelimitedCrypto";
import { PRODUCT_SURFACE_REVISION } from "../src/types/generated/PRODUCT_SURFACE_REVISION";

const RECOVERY_CODE = "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567";
const ADMISSION_INTENT_KEY = "agentsassemble.roomAdmissionIntent.v1";

function knownUserPreflight(roomId: string) {
  return {
    status: "known_user",
    can_auto_join: true,
    room_id: roomId,
    room_label: roomId,
    invite_scope: "room",
    participant: {
      participant_id: "guest-1",
      display_name: "Guest",
      avatar_image_url: "",
    },
    operator: false,
  };
}

async function admittedPayload(
  request: { request_id: string; client_id: string },
  roomId: string,
  sessionToken: string
) {
  const fields = [String(PRODUCT_SURFACE_REVISION), "streams", "actions"];
  const digest = await sha256Hex(
    lengthDelimitedTranscript("agentsassemble.server-product-surface.v1", fields)
  );
  return {
    status: "admitted",
    request_id: request.request_id,
    session_token: sessionToken,
    agent_id: "guest-1",
    display_name: "Guest",
    meeting_id: roomId,
    invite_scope: "room",
    participant_type: "human",
    client_type: "browser",
    provider_kind: "human",
    connection_kind: "browser",
    expires_at: "2099-01-01T00:00:00Z",
    room_label: roomId,
    room_topic: "",
    room_created_at: "2026-08-28T00:00:00Z",
    owner_display_name: "Host",
    owner_id: "local-user",
    stable_identity: false,
    operator: false,
    client_id: request.client_id,
    guide: {
      welcome: "Welcome",
      how_to: [],
      etiquette: [],
      session: { expires_in_seconds: 3600, rejoin: "Use the same invite." },
    },
    server_id: "11111111-1111-4111-8111-111111111111",
    authority_lineage_id: "22222222-2222-4222-8222-222222222222",
    server_product_surface: {
      revision: PRODUCT_SURFACE_REVISION,
      digest,
      http_routes: [],
      websocket_streams: [],
      websocket_actions: [],
    },
  };
}

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
      body: JSON.stringify(knownUserPreflight("room-1")),
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
    if (joinBodies.length === 3) {
      await route.fulfill({
        status: 403,
        contentType: "application/json",
        body: JSON.stringify({
          code: "admission_session_unavailable",
          error: "The admission session is no longer available.",
        }),
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
  await expect.poll(() => joinBodies.length).toBe(3);
  await expect
    .poll(() => page.evaluate((key) => sessionStorage.getItem(key), ADMISSION_INTENT_KEY))
    .toBeNull();

  expect(preflightCount).toBe(1);
  expect(joinBodies[1]).toEqual(joinBodies[0]);
  expect(joinBodies[2]).toEqual(joinBodies[0]);
});

test("repairs a completed admission cleanup before a different invite", async ({
  page,
}) => {
  await page.addInitScript((intentKey) => {
    const removeItem = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (key) {
      if (
        key === intentKey &&
        localStorage.getItem("agentsassemble.test.blockIntentRemoval") === "1"
      ) {
        throw new Error("storage unavailable");
      }
      removeItem.call(this, key);
    };
  }, ADMISSION_INTENT_KEY);
  const preflightTokens: string[] = [];
  await page.route("**/api/room-invite/admission", async (route) => {
    const request = route.request().postDataJSON() as { invite_token: string };
    preflightTokens.push(request.invite_token);
    const roomId = request.invite_token === "invite-a" ? "room-a" : "room-b";
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify(
        request.invite_token === "invite-a"
          ? knownUserPreflight(roomId)
          : {
              status: "profile_required",
              can_auto_join: false,
              room_id: roomId,
              room_label: roomId,
              invite_scope: "room",
            }
      ),
    });
  });
  await page.route("**/api/room-invite/join", async (route) => {
    const request = route.request().postDataJSON() as {
      request_id: string;
      client_id: string;
    };
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify(await admittedPayload(request, "room-a", "aas1.session-a")),
    });
  });

  await page.goto("/");
  await page.evaluate(() =>
    localStorage.setItem("agentsassemble.test.blockIntentRemoval", "1")
  );
  await page.goto("/join?token=invite-a");
  await expect(page).toHaveURL(/\/join$/);
  await expect
    .poll(() => page.evaluate((key) => sessionStorage.getItem(key), ADMISSION_INTENT_KEY))
    .not.toBeNull();

  await page.evaluate(() =>
    localStorage.setItem("agentsassemble.test.blockIntentRemoval", "0")
  );
  await page.goto("/join?token=invite-b");

  await expect.poll(() => preflightTokens).toEqual(["invite-a", "invite-b"]);
  await expect
    .poll(() => page.evaluate((key) => sessionStorage.getItem(key), ADMISSION_INTENT_KEY))
    .toBeNull();
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
