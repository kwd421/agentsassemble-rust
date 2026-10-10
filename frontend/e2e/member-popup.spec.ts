import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { HUMAN_INVITE_JOIN_CODE_BYTES, HUMAN_INVITE_JOIN_CODE_PREFIX } from "../src/types/generated/HUMAN_INVITE_WIRE";
import { chromium, firefox, webkit, expect, test } from "@playwright/test";

const central = "https://agentsassemble-identity-directory.seinel.workers.dev";
const entry = `${central}/member-join?protocol=secure_admission_v1&server_id=server&registration_epoch=epoch&token=${HUMAN_INVITE_JOIN_CODE_PREFIX}${Buffer.alloc(HUMAN_INVITE_JOIN_CODE_BYTES).toString("base64url")}`;
for (const [name, engine] of Object.entries({ chromium, firefox, webkit })) {
  test(`${name}: private context, blocked retry, COOP-severed return, explicit signup and consent`, async () => {
    const browser = await test.step("launch isolated browser", () => engine.launch({ timeout: 15_000, headless: process.env.PLAYWRIGHT_HEADLESS !== "false" }));
    const context = await test.step("create private context", () => browser.newContext());
    try {
      let registered = false, exchanges = 0, deviceId = "";
      await context.route(`${central}/**`, async route => {
        const url = new URL(route.request().url());
        if (url.pathname === "/v1/auth/google/web/verify-start") {
          const body = route.request().postDataJSON(); deviceId = body.device_id;
          const authorization = new URL("https://accounts.google.com/o/oauth2/v2/auth");
          authorization.search = new URLSearchParams({ response_type: "code", scope: "openid profile", code_challenge_method: "S256", client_id: "fixture-client", nonce: "fixture-nonce", state: body.state, code_challenge: body.code_challenge, redirect_uri: `${central}/` }).toString();
          return route.fulfill({ json: { handoff_id: "goh_fixture", state: body.state, expires_at: Math.floor(Date.now() / 1000) + 300, authorization_url: authorization.toString() } });
        }
        if (url.pathname === "/v1/auth/google/web/verify-complete" || url.pathname === "/v1/auth/google/web/register") {
          exchanges++;
          if (url.pathname.endsWith("register")) registered = true;
          return route.fulfill({ json: registered ? { status: "complete", person: { person_id: "person", identity_kind: "google", display_name: "테스트" },
            session: { token: "fixture-session", device_id: deviceId, expires_at: Math.floor(Date.now() / 1000) + 300 } } : { status: "absent" } });
        }
        if (url.pathname.endsWith("/member-preview")) return route.fulfill({ json: {
          server_id: "server", registration_epoch: "epoch", label: "테스트 서버", endpoint_origin: "https://host.test", endpoint_generation: 1,
          protocol: "secure_admission_v1", mode: "event_secure_v1", host_key_fingerprint: "fixture", host_public_key_jwk: {},
        } });
        if (url.pathname.startsWith("/v1/")) return route.fulfill({ status: 401, json: { error: "unauthorized" } });
        const file = url.pathname.startsWith("/assets/") ? url.pathname.slice(1) : "index.html";
        const contentType = file.endsWith(".js") ? "text/javascript" : file.endsWith(".css") ? "text/css" : "text/html";
        return route.fulfill({ body: await readFile(resolve("dist", file)), contentType, headers: { "Cross-Origin-Opener-Policy": "same-origin-allow-popups" } });
      });
      // Local provider double deliberately creates a new browsing context group.
      await context.route("https://accounts.google.com/**", route => {
        const state = new URL(route.request().url()).searchParams.get("state");
        return route.fulfill({ contentType: "text/html", headers: { "Cross-Origin-Opener-Policy": "same-origin" },
          body: `<a href="${central}/?code=fixture-code&state=${state}">Continue</a>` });
      });
      const page = await context.newPage();
      await test.step("open trusted invite", () => page.goto(entry));
      await expect(page.getByRole("button", { name: "Google로 계속" })).toBeVisible();
      expect(new URL(page.url()).search).toBe("");
      await page.evaluate(() => {
        const original = window.open; window.open = () => { window.open = original; return null; };
      });
      await page.getByRole("button", { name: "Google로 계속" }).click();
      await expect(page.getByText("로그인 창이 차단됐어요. 팝업을 허용한 뒤 Google로 다시 시도해 주세요.")).toBeVisible();
      const popupPromise = context.waitForEvent("page");
      await page.getByRole("button", { name: "Google로 다시 시도" }).click();
      const popup = await popupPromise;
      await test.step("return from COOP provider", () => popup.getByRole("link", { name: "Continue" }).click());
      await expect(popup.getByRole("button", { name: "새로 가입", exact: true })).toBeVisible();
      expect(await popup.evaluate(() => window.opener === null)).toBe(true);
      expect(new URL(popup.url()).search).toBe("");
      expect(registered).toBe(false); expect(exchanges).toBe(1);
      await popup.getByRole("button", { name: "새로 가입", exact: true }).click();
      await expect(page.getByRole("button", { name: "참가하기", exact: true })).toBeVisible();
      expect(registered).toBe(true); expect(exchanges).toBe(2);
      const persistedInvite = await page.evaluate(prefix => [localStorage, sessionStorage].some(storage => Object.values(storage).some(value => String(value).includes(prefix))), HUMAN_INVITE_JOIN_CODE_PREFIX);
      expect(persistedInvite).toBe(false);
    } finally { await context.close(); await browser.close(); }
  });
}
