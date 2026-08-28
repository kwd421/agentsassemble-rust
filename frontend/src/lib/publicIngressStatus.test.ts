import { describe, expect, it } from "vitest";
import { parsePublicIngressStatus } from "./publicIngressStatus";

const managed = {
  mode: "managed",
  public_url: "",
  stable_url: "",
  tunnel: {
    available: true,
    running: false,
    phase: "stopped",
    public_url: "",
    local_url: "http://127.0.0.1:43123",
    stable_phase: "unconfigured",
  },
};

describe("parsePublicIngressStatus", () => {
  it("accepts the current fixed and managed lifecycle states", () => {
    expect(
      parsePublicIngressStatus({
        mode: "unconfigured",
        public_url: "",
        stable_url: "",
        tunnel: {
          available: false,
          running: false,
          phase: "stopped",
          public_url: "",
          local_url: "",
          stable_phase: "unconfigured",
        },
      }).mode
    ).toBe("unconfigured");
    expect(
      parsePublicIngressStatus({
        mode: "manual",
        public_url: "https://public.example.com",
        stable_url: "",
        tunnel: {
          available: false,
          running: false,
          phase: "stopped",
          public_url: "https://public.example.com",
          local_url: "",
          stable_phase: "unconfigured",
        },
      }).mode
    ).toBe("manual");
    expect(
      parsePublicIngressStatus({
        mode: "manual",
        public_url: "https://foo.localhost",
        stable_url: "",
        tunnel: {
          available: false,
          running: false,
          phase: "stopped",
          public_url: "https://foo.localhost",
          local_url: "",
          stable_phase: "unconfigured",
        },
      }).public_url
    ).toBe("https://foo.localhost");
    expect(parsePublicIngressStatus(managed)).toEqual(managed);
    expect(
      parsePublicIngressStatus({
        ...managed,
        public_url: "https://direct.example.com",
        stable_url: "https://stable.example.com",
        tunnel: {
          ...managed.tunnel,
          running: true,
          phase: "running",
          public_url: "https://direct.example.com",
          stable_phase: "ready",
        },
      }).stable_url
    ).toBe("https://stable.example.com");
    expect(
      parsePublicIngressStatus({
        ...managed,
        tunnel: { ...managed.tunnel, stable_phase: "ready" },
      }).tunnel.stable_phase
    ).toBe("ready");
  });

  it.each([
    ["unknown top field", { ...managed, ignored: true }],
    ["unknown tunnel field", { ...managed, tunnel: { ...managed.tunnel, ignored: true } }],
    ["coerced availability", { ...managed, tunnel: { ...managed.tunnel, available: 1 } }],
    ["running flag mismatch", { ...managed, tunnel: { ...managed.tunnel, running: true } }],
    [
      "active unavailable phase",
      { ...managed, tunnel: { ...managed.tunnel, available: false, running: true, phase: "starting" } },
    ],
    [
      "top and direct URL mismatch",
      {
        ...managed,
        public_url: "https://one.example.com",
        tunnel: { ...managed.tunnel, running: true, phase: "running", public_url: "https://two.example.com" },
      },
    ],
    [
      "non-running direct URL",
      { ...managed, public_url: "https://one.example.com", tunnel: { ...managed.tunnel, public_url: "https://one.example.com" } },
    ],
    [
      "non-ready stable URL",
      { ...managed, stable_url: "https://stable.example.com" },
    ],
    [
      "ready stable target without direct target",
      { ...managed, stable_url: "https://stable.example.com", tunnel: { ...managed.tunnel, stable_phase: "ready" } },
    ],
    [
      "active empty ready state",
      { ...managed, tunnel: { ...managed.tunnel, running: true, phase: "starting", stable_phase: "ready" } },
    ],
    ["error without message", { ...managed, tunnel: { ...managed.tunnel, phase: "error" } }],
    ["stable failure without message", { ...managed, tunnel: { ...managed.tunnel, stable_phase: "failed" } }],
    [
      "trailing-dot localhost origin",
      {
        mode: "manual",
        public_url: "https://localhost.",
        stable_url: "",
        tunnel: {
          available: false,
          running: false,
          phase: "stopped",
          public_url: "https://localhost.",
          local_url: "",
          stable_phase: "unconfigured",
        },
      },
    ],
    [
      "noncanonical public URL",
      { ...managed, public_url: "https://public.example.com/", tunnel: { ...managed.tunnel, running: true, phase: "running", public_url: "https://public.example.com/" } },
    ],
    ["nonlocal runtime URL", { ...managed, tunnel: { ...managed.tunnel, local_url: "http://localhost:43123" } }],
  ])("rejects %s", (_label, value) => {
    expect(() => parsePublicIngressStatus(value)).toThrow(
      "공개 ingress 상태 응답 계약이 올바르지 않습니다."
    );
  });
});
