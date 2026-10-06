// @vitest-environment-options {"url":"https://host.test/"}
import { beforeEach, describe, expect, it, vi } from "vitest";
import { encodeBase64Url } from "../base64Url";
import {
  centralMemberEntryUrl, consumeCentralMemberRequest, consumeMemberReturn,
  createMemberHandoff, exactMemberOrigin, memberCallbackUrl, memberTargetRequest,
  parseMemberGrant, purgeMemberHandoffs, storeMemberHandoff, nativeMemberCallbackUrl, retainedNativeMemberCredential,
} from "./memberConnect";

const challenge = { server_id: "server", registration_epoch: "epoch", challenge_hash: "a".repeat(43),
  challenge_id: "challenge", expires_at: Math.floor(Date.now() / 1000) + 300 };
const grant = { server_id: "server", registration_epoch: "epoch", grant_token: `aamg1.${"b".repeat(43)}`,
  expires_at: challenge.expires_at, endpoint_origin: window.location.origin, endpoint_generation: 3 };
const record = () => createMemberHandoff(challenge, "private-invite", "room");
const fragment = (value: unknown) => `#central-member=${encodeBase64Url(new TextEncoder().encode(JSON.stringify(value)))}`;

describe("member handoff boundary", () => {
  beforeEach(() => { sessionStorage.clear(); window.history.replaceState({}, "", "/join"); });

  it.each(["http://host.test", "https://host.test/", "https://u:p@host.test", "https://host.test/path",
    "https://host.test?x=1", "https://host.test#x", "https://host.test:443", "https://HOST.test"])("rejects non-exact HTTPS origin %s", (origin) => {
    expect(() => exactMemberOrigin(origin)).toThrow();
  });

  it("keeps invite and challenge id on host, preserves only the tuple across central login", () => {
    const pending = record(); storeMemberHandoff(pending);
    const request = memberTargetRequest(pending);
    const url = centralMemberEntryUrl("https://central.test", request);
    expect(url).not.toContain("private-invite");
    window.history.replaceState({}, "", `/member-join${new URL(url).hash}`);
    expect(consumeCentralMemberRequest()).toEqual(request);
    expect(window.location.hash).toBe("");
    window.history.replaceState({}, "", "/?code=google-code&state=google-state");
    expect(consumeCentralMemberRequest()).toEqual(request);
    expect(window.location.search).toContain("code=google-code");
  });

  it("removes a malformed fragment before decoding or reading storage", () => {
    window.history.replaceState({}, "", `/join#central-member=${encodeBase64Url(new TextEncoder().encode("invalid-json"))}`);
    const decode = vi.spyOn(TextDecoder.prototype, "decode").mockImplementation(() => {
      expect(window.location.hash).toBe(""); throw new Error("invalid encoding");
    });
    expect(consumeMemberReturn()?.error).toBeTruthy();
    expect(decode).toHaveBeenCalledOnce();
    expect(window.location.hash).toBe("");
    decode.mockRestore();
  });

  it("returns a valid grant once and keeps another pending invite separate", () => {
    const pending = record(); const other = record();
    storeMemberHandoff(pending); storeMemberHandoff(other);
    const hash = new URL(memberCallbackUrl(grant, memberTargetRequest(pending))).hash;
    window.history.replaceState({}, "", `/join${hash}`);
    expect(consumeMemberReturn()).toEqual({ record: pending, grant });
    expect(sessionStorage.getItem(`agentsassemble.memberHandoff.v1:${other.handoff_state}`)).toBeTruthy();
    window.history.replaceState({}, "", `/join${hash}`);
    expect(consumeMemberReturn()?.grant).toBeUndefined();
  });

  it("takes and deletes the correlated record even on an invalid grant", () => {
    const pending = record(); storeMemberHandoff(pending);
    window.history.replaceState({}, "", `/join${fragment({ ...memberTargetRequest(pending), grant: { ...grant, grant_token: "bad" } })}`);
    const storage = vi.spyOn(Storage.prototype, "getItem");
    const result = consumeMemberReturn();
    expect(result?.record).toEqual(pending);
    expect(result?.error).toBeTruthy();
    expect(storage).toHaveBeenCalled();
    expect(Object.keys(sessionStorage)).toHaveLength(0);
    expect(window.location.hash).toBe(""); storage.mockRestore();
  });

  it("refuses missing, mismatched and expired records without replaying a grant", () => {
    for (const mode of ["missing", "mismatch", "expired"]) {
      const pending = record();
      if (mode !== "missing") storeMemberHandoff(pending);
      if (mode === "expired") sessionStorage.setItem(`agentsassemble.memberHandoff.v1:${pending.handoff_state}`, JSON.stringify({ ...pending, expires_at: 1 }));
      const request = { ...memberTargetRequest(pending), ...(mode === "mismatch" ? { registration_epoch: "other" } : {}) };
      window.history.replaceState({}, "", `/join${fragment({ ...request, grant })}`);
      const result = consumeMemberReturn();
      expect(result?.error).toBeTruthy(); expect(result?.grant).toBeUndefined();
      expect(Object.keys(sessionStorage)).toHaveLength(0);
    }
  });

  it("uses only the fixed callback path and validates grant identity/expiry/generation", () => {
    const request = memberTargetRequest(record());
    const valid = { ...grant, endpoint_origin: "https://host.test" };
    expect(parseMemberGrant(valid, request)).toEqual(valid);
    expect(new URL(memberCallbackUrl(valid, request)).pathname).toBe("/join");
    for (const invalid of [{ ...valid, server_id: "other" }, { ...valid, expires_at: 1 }, { ...valid, endpoint_generation: 0 }]) {
      expect(() => parseMemberGrant(invalid, request)).toThrow();
    }
  });

  it("purges only expired member records", () => {
    const pending = record(); storeMemberHandoff(pending);
    sessionStorage.setItem("agentsassemble.memberHandoff.v1:old", JSON.stringify({ expires_at: 1 }));
    sessionStorage.setItem("unrelated", "preserve"); purgeMemberHandoffs();
    expect(sessionStorage.getItem("agentsassemble.memberHandoff.v1:old")).toBeNull();
    expect(sessionStorage.getItem(`agentsassemble.memberHandoff.v1:${pending.handoff_state}`)).toBeTruthy();
    expect(sessionStorage.getItem("unrelated")).toBe("preserve");
  });
});

it("keeps connect purpose across a credential-free handoff and rejects admission tokens",()=>{
  const pending={...createMemberHandoff(challenge,"",""),purpose:"connect" as const};
  const request=memberTargetRequest(pending);
  expect(request.purpose).toBe("connect");
  expect(()=>parseMemberGrant(grant,request)).toThrow();
  const issued={...grant,grant_token:`aamc1.${"c".repeat(43)}`};
  storeMemberHandoff(pending);
  window.history.replaceState({},"",new URL(memberCallbackUrl(issued,request)).pathname+new URL(memberCallbackUrl(issued,request)).hash);
  expect(consumeMemberReturn()?.record?.purpose).toBe("connect");
});

it("validates native connect origin and retains only its scoped browser credential after removing the fragment",()=>{
  const pending={...createMemberHandoff(challenge,"",""),purpose:"connect" as const};
  const issued={...grant,grant_token:`aamc1.${"c".repeat(43)}`};
  const credential=`aad1_${"A".repeat(43)}`;
  const url=nativeMemberCallbackUrl(pending,issued,credential);
  localStorage.setItem("agentsassemble.browserCredential.v1","existing-browser");
  window.history.replaceState({},"",new URL(url).pathname+new URL(url).hash);
  const result=consumeMemberReturn();
  expect(result?.record?.purpose).toBe("connect");
  expect(result?.browserCredential).toBe(credential);
  expect(window.location.hash).toBe("");
  expect(retainedNativeMemberCredential()).toBe(credential);
  expect(localStorage.getItem("agentsassemble.browserCredential.v1")).toBe("existing-browser");
  const wrong=nativeMemberCallbackUrl(pending,{...issued,endpoint_origin:"https://other.test"},credential);
  window.history.replaceState({},"","/join"+new URL(wrong).hash);
  expect(consumeMemberReturn()?.error).toBeTruthy();
  expect(window.location.hash).toBe("");
});
