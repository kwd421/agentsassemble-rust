import { PROTOCOL_VERSION } from "../src/types/generated/PROTOCOL_VERSION";
import { roomFixture } from "../src/test/room";
import { participantFixture } from "../src/test/participant";
import { TEST_SERVER_PRODUCT_SURFACE } from "../src/test/serverProductSurface";
import { expect, test } from "@playwright/test";
import {
  lengthDelimitedTranscript,
  sha256Hex,
} from "../src/lib/lengthDelimitedCrypto";
import { PRODUCT_SURFACE_REVISION } from "../src/types/generated/PRODUCT_SURFACE_REVISION";

const RECOVERY_CODE = "ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567";
const ADMISSION_INTENT_KEY = "agentsassemble.roomAdmissionIntent.v1";
const GUEST_SESSION_KEY = "agentsassemble.roomGuestSession.v1";

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
      session: {
        expires_in_seconds: 3600,
        rejoin:
          "This session cannot be renewed after it expires; ask the host for a new invite link.",
      },
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

  await expect(page.getByRole("region", { name: /에 초대받았어요/ })).toBeVisible();
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
  await page.addInitScript((intentKey) => {
    const removeItem = Storage.prototype.removeItem;
    Storage.prototype.removeItem = function (key) {
      if (
        key === intentKey &&
        localStorage.getItem("agentsassemble.test.blockTerminalIntentRemoval") === "1"
      ) {
        throw new Error("storage unavailable");
      }
      removeItem.call(this, key);
    };
  }, ADMISSION_INTENT_KEY);
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

  await page.goto("/");
  await page.evaluate(() =>
    localStorage.setItem("agentsassemble.test.blockTerminalIntentRemoval", "1")
  );
  await page.goto("/join?token=invite-token");
  await expect(page.getByRole("region", { name: "입장 재시도" })).toBeVisible();
  await expect.poll(() => joinBodies.length).toBe(1);

  await page.reload();
  await expect.poll(() => joinBodies.length).toBe(2);

  await page.reload();
  await expect.poll(() => joinBodies.length).toBe(3);
  await expect(page.getByRole("region", { name: "입장 재시도" })).toBeVisible();
  expect(
    await page.evaluate((key) => {
      const raw = sessionStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    }, ADMISSION_INTENT_KEY)
  ).toMatchObject({
    state: "settled",
    outcome: "terminal",
    terminalCode: "admission_session_unavailable",
  });

  await page.reload();
  await expect(page.getByRole("region", { name: "입장 확인 재시도" })).toBeVisible();
  expect(joinBodies).toHaveLength(3);

  await page.evaluate(() =>
    localStorage.setItem("agentsassemble.test.blockTerminalIntentRemoval", "0")
  );
  await page.getByRole("button", { name: "다시 시도", exact: true }).click();
  await expect
    .poll(() => page.evaluate((key) => sessionStorage.getItem(key), ADMISSION_INTENT_KEY))
    .toBeNull();

  expect(preflightCount).toBe(1);
  expect(joinBodies).toHaveLength(3);
  expect(joinBodies[1]).toEqual(joinBodies[0]);
  expect(joinBodies[2]).toEqual(joinBodies[0]);
});

test("repairs durable settlement after completed session custody is removed", async ({
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
  const preflightSessionTokens: string[] = [];
  await page.route("**/api/room-invite/admission", async (route) => {
    const request = route.request().postDataJSON() as {
      invite_token: string;
      session_token?: string;
    };
    preflightTokens.push(request.invite_token);
    preflightSessionTokens.push(request.session_token || "");
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
  expect(
    await page.evaluate((key) => {
      const raw = sessionStorage.getItem(key);
      return raw ? JSON.parse(raw) : null;
    }, ADMISSION_INTENT_KEY)
  ).toMatchObject({ state: "settled", outcome: "completed_session" });

  await page.evaluate(() =>
    localStorage.setItem("agentsassemble.test.blockIntentRemoval", "0")
  );
  await page.evaluate((sessionKey) => localStorage.removeItem(sessionKey), GUEST_SESSION_KEY);
  await page.goto("/join?token=invite-b");

  await expect.poll(() => preflightTokens).toEqual(["invite-a", "invite-b"]);
  expect(preflightSessionTokens).toEqual(["", ""]);
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


test("keeps the composer usable at 768–1100px with members open", async ({ page }) => {
  await page.route("**/join?token=layout-invite", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()).replace("<html", `<html data-agentsassemble-build="${"b".repeat(64)}"`) });
  });
  await page.route("**/api/runtime/version", route => route.fulfill({ json: {
    frontend_build_id: "b".repeat(64), protocol_version: PROTOCOL_VERSION,
  } }));
  await page.route("**/api/side-chat?*", route => route.fulfill({ status: 503, json: {} }));
  await page.route("**/api/room-invite/admission", route => route.fulfill({ json: knownUserPreflight("general") }));
  await page.route("**/api/room-invite/join", async route => route.fulfill({
    json: { ...await admittedPayload(route.request().postDataJSON(), "general", "aas1.layout-session"), server_product_surface: TEST_SERVER_PRODUCT_SURFACE },
  }));
  await page.route("**/api/session-tickets/socket", route => route.fulfill({ json: { ticket: "a".repeat(64), ttl_seconds: 30 } }));
  await page.routeWebSocket("**/ws?*", socket => {
    socket.onMessage(raw => {
      const request = JSON.parse(String(raw));
      if (request.op !== "subscribe") return;
      socket.send(JSON.stringify({ op: "subscribed", protocol_version: 1, streams: request.streams,
        room_id: "general", principal_id: "guest", participant_id: "guest-1",
        server_surface_revision: TEST_SERVER_PRODUCT_SURFACE.revision, snapshot_cursor: 0, catchup_high_water: 0 }));
      socket.send(JSON.stringify({ op: "provider_catalog_updated", catalog: { status: "ready", catalog_revision: "layout", providers: [] } }));
      socket.send(JSON.stringify({ op: "provider_request_snapshot", request: null }));
      socket.send(JSON.stringify({ op: "snapshot", stream: "room_events", room: roomFixture(),
        room_settings: { settings_revision: "layout", label: "General", topic: "",
          appearance: { banner_preset: "default", banner_image_url: "", icon_image_url: "", icon_label: "G", invite_scope: "room" },
          conversation_mode: "ordered", tool_mode: "chat", ordered_exclude_previous_speaker: true, channels: [] },
        participants: [participantFixture({ participant_id: "guest-1", display_name: "Guest" })],
        agent_sessions: [], active_turns: [], events: [], oldest_seq: 0, last_seq: 0,
        has_more_before: false, resume_gap: false, snapshot_mode: "initial",
        capabilities: { "message.send": true, "message.modify": true, "participant.leave": true,
          "participant.mute": false, "room.history": true, "room.manage": false, "agent.control": false,
          "bridge.publish": false, "room.random": true, "room.vote.summary": true } }));
    });
  });
  await page.route("**/api/user-profile", route => route.fulfill({ status: 503, json: {} }));
  await page.route("**/api/room-settings?*", route => route.fulfill({ status: 503, json: {} }));
  await page.addInitScript(() => localStorage.setItem("agentsassemble.sidebar.width.v1", "420"));
  await page.goto("/join?token=layout-invite");
  const input = page.locator('textarea[aria-label="채팅 입력"]');
  await expect(input).toBeVisible();
  for (const width of [768, 800, 814, 900, 1024, 1100]) {
    await page.setViewportSize({ width, height: 800 });
    const panel = page.getByTestId("room-right-panel");
    await expect(panel).toBeVisible();
    await expect(panel.getByRole("region", { name: "동반 AI 초대" })).toBeVisible();
    await expect(panel).toHaveCSS("position", "absolute");
    expect((await input.boundingBox())!.width).toBeGreaterThan(0);
    await page.getByRole("button", { name: "멤버 목록 닫기" }).click();
    await expect(panel).toHaveCount(0);
    await input.fill(`폭 ${width} 입력 확인`);
    await expect(input).toHaveValue(`폭 ${width} 입력 확인`);
    await page.getByRole("button", { name: "사이드챗 열기" }).click();
    await expect(page.getByRole("complementary", { name: "사이드챗 패널" })).toHaveCSS("position", "absolute");
    expect((await input.boundingBox())!.width).toBeGreaterThan(0);
    await page.getByRole("button", { name: "사이드챗 닫기" }).click();
    await page.getByRole("button", { name: "멤버 목록 토글" }).click();
  }
});
