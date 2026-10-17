// @vitest-environment-options {"url":"https://tauri.localhost/"}
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const native=vi.hoisted(()=>({open:vi.fn()}));
vi.mock("../desktopBridge",async original=>({...await original<typeof import("../desktopBridge")>(),isDesktopWebview:()=>true,openDesktopCentralOwnedServer:native.open}));
import { bootstrapCentral, openCentralMemberServer, parseRetryAfterMs, previewCentralMember, saveSession } from "./identity";
import { pendingMemberSnapshot, selectRemoteMember } from "../remote/remoteWorkspace";

const person={person_id:"person",display_name:"이름",identity_kind:"google" as const};
const server={server_id:"host",registration_epoch:"epoch",relation:"member" as const,alias:"친구 컴퓨터",host_key_fingerprint:"fingerprint",endpoint:{origin:"https://host.test",generation:1,mode:"event_secure_v1",protocol:"secure_admission_v1",status:"published" as const}};
const fetcher=vi.fn();
beforeEach(async()=>{
  localStorage.clear();sessionStorage.clear();native.open.mockReset();fetcher.mockReset();
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL","https://central.test");
  const pair=await crypto.subtle.generateKey({name:"ECDSA",namedCurve:"P-256"},false,["sign","verify"]);
  const device={deviceId:"device",privateKey:pair.privateKey,publicJwk:{}};
  vi.stubGlobal("indexedDB",{open:()=>{
    const request={result:{close(){},transaction:()=>({objectStore:()=>({get:()=>{
      const read={result:device,onsuccess(){}};queueMicrotask(()=>read.onsuccess());return read;
    }})})},onsuccess(){}};queueMicrotask(()=>request.onsuccess());return request;
  }});
  vi.stubGlobal("fetch",fetcher);
  saveSession({person,session:{token:"central-fixture",device_id:"device",expires_at:9_999_999_999}});
});
afterEach(()=>{vi.unstubAllEnvs();vi.unstubAllGlobals();localStorage.clear();sessionStorage.clear();});
it("native reconnect retains a trusted entry until consent without host navigation or credentials", async () => {
  const href = window.location.href;
  await openCentralMemberServer(server);
  expect(pendingMemberSnapshot()).toEqual({ server_id: "host", registration_epoch: "epoch" });
  expect(fetcher).not.toHaveBeenCalled();
  expect(native.open).not.toHaveBeenCalled();
  expect(window.location.href).toBe(href);
  selectRemoteMember(null);
  expect(window.location.href).toBe(href);
});
it("member bootstrap exposes only the approved member fields",async()=>{
  fetcher.mockResolvedValue(new Response(JSON.stringify({person,server_time:1,servers:[{...server,host_os:"macos",default_name:"private",member_count:9,room_list:["private"],host_public_key_jwk:{x:"private"},endpoint:{...server.endpoint,lease_expires_at:999}}]})));
  const result=await bootstrapCentral();
  expect(result?.servers[0]).toEqual({...server,icon:undefined,host_public_key_jwk:{x:"private"}});
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).not.toMatch(/private|macos|lease_expires/);
});

it("rejects a legacy endpoint without a downgrade", async () => {
  await expect(openCentralMemberServer({ ...server, endpoint: { ...server.endpoint, mode: "legacy_lease" } })).rejects.toThrow("서버 앱을 업데이트해 주세요");
  expect(fetcher).not.toHaveBeenCalled(); expect(native.open).not.toHaveBeenCalled();
});

it("parses Retry-After delay seconds and HTTP dates", () => {
  expect(parseRetryAfterMs("120")).toBe(120_000);
  expect(parseRetryAfterMs("0")).toBe(0);
  expect(parseRetryAfterMs(null)).toBeNull();
  expect(parseRetryAfterMs("")).toBeNull();
  expect(parseRetryAfterMs("not-a-date")).toBeNull();
  expect(parseRetryAfterMs("-5")).toBeNull();
  const future = new Date(Date.now() + 90_000).toUTCString();
  const wait = parseRetryAfterMs(future);
  expect(wait).not.toBeNull();
  expect(wait as number).toBeGreaterThan(0);
  expect(wait as number).toBeLessThanOrEqual(90_000);
});

it("carries the server's Retry-After on a member-preview 429", async () => {
  fetcher.mockResolvedValue(Response.json({ error: { code: "rate_limited", message: "slow" } },
    { status: 429, headers: { "Retry-After": "120" } }));
  const target = { server_id: "host", registration_epoch: "epoch", challenge_hash: "x", handoff_state: "y" };
  await expect(previewCentralMember(target))
    .rejects.toMatchObject({ status: 429, code: "rate_limited", retryAfterMs: 120_000 });
});

it("omits the wait on a 429 without a Retry-After header", async () => {
  const { resetCentralRequestGovernorForTests } = await import("./centralRequestGovernor");
  fetcher.mockResolvedValue(Response.json({ error: { code: "rate_limited", message: "slow" } }, { status: 429 }));
  const target = { server_id: "host", registration_epoch: "epoch", challenge_hash: "x", handoff_state: "y" };
  await expect(previewCentralMember(target))
    .rejects.toMatchObject({ status: 429, code: "rate_limited" });
  // The governor would otherwise throttle the immediate second attempt;
  // reset it to assert the server payload shape in isolation.
  resetCentralRequestGovernorForTests();
  try {
    await previewCentralMember(target);
    expect.unreachable();
  } catch (error) {
    expect(error).toMatchObject({ status: 429 });
    expect("retryAfterMs" in (error as object)).toBe(false);
  }
});
