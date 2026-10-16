import { beforeEach, describe, expect, it } from "vitest";
import {
  clearMemberAnchors,
  loadMemberAnchor,
  loadMemberAnchors,
  removeMemberAnchor,
  removeMemberAnchorRoom,
  saveMemberAnchor,
  type MemberHostAnchor,
} from "./memberAnchors";

const anchor = (overrides: Partial<MemberHostAnchor> = {}): MemberHostAnchor => ({
  server_id: "server",
  registration_epoch: "epoch",
  host_key_fingerprint: "fingerprint",
  endpoint_origin: "https://host.test",
  endpoint_generation: 3,
  rooms: [{ room_id: "general", name: "일반" }],
  hidden: false,
  updated_at: 1_700_000_000,
  ...overrides,
});

describe("member host anchors", () => {
  beforeEach(() => { window.localStorage.clear(); });

  it("round-trips a per-host anchor without any session material", () => {
    saveMemberAnchor("person", anchor());
    expect(loadMemberAnchor("person", "server")).toEqual(anchor());
    const raw = window.localStorage.getItem("agentsassemble.memberAnchors.v1:person") || "";
    expect(raw).not.toMatch(/aam[cg]1|aad1_|session_token|grant_token|private/i);
  });

  it("rejects anchors carrying bearer, grant or key material", () => {
    for (const smuggled of [
      { session_token: "aaos1.x" },
      { grant_token: "aamg1.x" },
      { browser_credential: "aad1_x" },
      { private_key: "secret" },
    ]) {
      expect(() => saveMemberAnchor("person", { ...anchor(), ...smuggled } as MemberHostAnchor)).toThrow();
    }
    expect(loadMemberAnchors("person")).toEqual([]);
    expect(window.localStorage.getItem("agentsassemble.memberAnchors.v1:person")).toBeNull();
  });

  it("rejects malformed rooms, origins and generations", () => {
    expect(() => saveMemberAnchor("person", anchor({ endpoint_origin: "http://host.test" }))).toThrow();
    expect(() => saveMemberAnchor("person", anchor({ endpoint_generation: 0 }))).toThrow();
    expect(() => saveMemberAnchor("person", anchor({ rooms: [{ room_id: "", name: "x" }] }))).toThrow();
    expect(() => saveMemberAnchor("person", anchor({
      rooms: Array.from({ length: 51 }, (_, index) => ({ room_id: `room-${index}`, name: "" })),
    }))).toThrow();
    expect(loadMemberAnchors("person")).toEqual([]);
  });

  it("keeps anchors isolated per account and clears them on logout", () => {
    saveMemberAnchor("person-a", anchor());
    saveMemberAnchor("person-b", anchor({ server_id: "other" }));
    expect(loadMemberAnchors("person-a")).toHaveLength(1);
    clearMemberAnchors("person-a");
    expect(loadMemberAnchors("person-a")).toEqual([]);
    expect(loadMemberAnchors("person-b")).toHaveLength(1);
    clearMemberAnchors("person-b");
  });

  it("removes one host without touching the others, and drops empty stores", () => {
    saveMemberAnchor("person", anchor());
    saveMemberAnchor("person", anchor({ server_id: "other", endpoint_origin: "https://other.test" }));
    removeMemberAnchor("person", "server");
    expect(loadMemberAnchors("person").map((entry) => entry.server_id)).toEqual(["other"]);
    removeMemberAnchor("person", "other");
    expect(window.localStorage.getItem("agentsassemble.memberAnchors.v1:person")).toBeNull();
  });

  it("removes a left room from the cached list without inventing rooms", () => {
    saveMemberAnchor("person", anchor({ rooms: [{ room_id: "general", name: "일반" }, { room_id: "random", name: "" }] }));
    removeMemberAnchorRoom("person", "server", "general");
    expect(loadMemberAnchor("person", "server")?.rooms).toEqual([{ room_id: "random", name: "" }]);
    removeMemberAnchorRoom("person", "server", "missing");
    expect(loadMemberAnchor("person", "server")?.rooms).toHaveLength(1);
  });
});
