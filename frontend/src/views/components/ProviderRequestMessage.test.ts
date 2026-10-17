import { describe, expect, it } from "vitest";
import { providerRequestOptionLabel } from "./ProviderRequestMessage";

describe("providerRequestOptionLabel", () => {
  it("labels a network deny rule as deny, never as allow", () => {
    const label = providerRequestOptionLabel({
      id: "network-policy-1",
      label: "Apply deny network rule",
      kind: "deny_policy",
      description: "blocked.test",
    });
    expect(label).toContain("거부");
    expect(label).not.toContain("허용");
  });

  it("keeps the allow rule label and stays neutral for unknown kinds", () => {
    const allow = providerRequestOptionLabel({
      id: "network-policy-0",
      label: "Apply allow network rule",
      kind: "allow_policy",
      description: "example.test",
    });
    expect(allow).toContain("허용");
    const unknown = providerRequestOptionLabel({
      id: "future-0",
      label: "Future",
      kind: "future_kind",
      description: "",
    });
    expect(unknown).toBe("선택");
  });
});
