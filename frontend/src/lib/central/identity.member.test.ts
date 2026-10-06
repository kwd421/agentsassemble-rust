// @vitest-environment-options {"url":"https://tauri.localhost/"}
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const native=vi.hoisted(()=>({open:vi.fn()}));
vi.mock("../desktopBridge",async original=>({...await original<typeof import("../desktopBridge")>(),isDesktopWebview:()=>true,openDesktopCentralOwnedServer:native.open}));
import { bootstrapCentral, openCentralMemberServer, saveSession } from "./identity";
import { getOrCreateBrowserCredential } from "../deviceIdentity";

const person={person_id:"person",display_name:"이름",identity_kind:"google" as const};
const server={server_id:"host",registration_epoch:"epoch",relation:"member" as const,alias:"친구 컴퓨터",host_key_fingerprint:"fingerprint",endpoint:{origin:"https://host.test",generation:1,status:"likely_online" as const}};
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
it("native reconnect signs only the connect route and keeps central credentials off the host",async()=>{
  const ordinary=getOrCreateBrowserCredential();
  const expires_at=Math.floor(Date.now()/1000)+300;
  fetcher.mockImplementation(async (url:string,init:RequestInit)=>{
    if(url.startsWith("https://host.test")) {
      expect(new Headers(init.headers).has("Authorization")).toBe(false);
      expect(new Headers(init.headers).get("X-Device-Token")).not.toBe(ordinary);
      return new Response(JSON.stringify({server_id:"host",registration_epoch:"epoch",challenge_id:"challenge",challenge_hash:"a".repeat(43),expires_at}));
    }
    expect(url).toBe("https://central.test/v1/servers/host/member-connect-grants");
    expect(JSON.parse(init.body as string)).toEqual({registration_epoch:"epoch",challenge_hash:"a".repeat(43),purpose:"connect"});
    return new Response(JSON.stringify({server_id:"host",registration_epoch:"epoch",grant_token:`aamc1.${"b".repeat(43)}`,endpoint_origin:"https://host.test",endpoint_generation:1,expires_at}),{status:201});
  });
  await openCentralMemberServer(server);
  expect(native.open).toHaveBeenCalledOnce();
  expect(native.open.mock.calls[0][0]).toMatch(/^https:\/\/host.test\/join#native-member=/);
  expect(getOrCreateBrowserCredential()).toBe(ordinary);
});
it("member bootstrap exposes only the approved member fields",async()=>{
  fetcher.mockResolvedValue(new Response(JSON.stringify({person,server_time:1,servers:[{...server,host_os:"macos",default_name:"private",member_count:9,room_list:["private"],host_public_key_jwk:{x:"private"},endpoint:{...server.endpoint,lease_expires_at:999}}]})));
  const result=await bootstrapCentral();
  expect(result?.servers[0]).toEqual({...server,icon:undefined});
  expect(localStorage.getItem("agentsassemble.centralDirectoryDisplay.v1")).not.toMatch(/private|macos|lease_expires/);
});

it("does not expose central errors or identifiers in the reconnect message",async()=>{
  fetcher.mockResolvedValue(new Response(JSON.stringify({error:{code:"internal_fixture",message:"private internal context"}}),{status:429}));
  await expect(openCentralMemberServer(server)).rejects.toThrow("이 컴퓨터에 다시 연결하지 못했어요. 잠시 뒤 다시 시도해 주세요.");
  expect(native.open).not.toHaveBeenCalled();
});
