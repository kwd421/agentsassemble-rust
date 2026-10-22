import { describe, expect, it } from "vitest";
import { distinctNames } from "./distinctNames";

describe("distinctNames", () => {
  it("keeps unique names and numbers shared ones in participant ID order", () => {
    const labels = distinctNames([
      { id: "c", name: "Ace" }, { id: "a", name: "ace" }, { id: "b", name: "Solo" },
    ]);
    expect(labels.get("a")).toBe("ace (1)");
    expect(labels.get("c")).toBe("Ace (2)");
    expect(labels.get("b")).toBe("Solo");
  });

  it("never reuses a label another participant or the viewer has", () => {
    const labels = distinctNames([
      { id: "a", name: "Foo" }, { id: "b", name: "Foo" }, { id: "c", name: "Foo (2)" }, { id: "d", name: "Me" },
    ], ["me"]);
    expect(labels.get("c")).toBe("Foo (2)");
    expect(labels.get("a")).toBe("Foo (1)");
    expect(labels.get("b")).toBe("Foo (3)");
    expect(labels.get("d")).toBe("Me (1)");
  });

  it("compares names the way the mention composer writes them", () => {
    const labels = distinctNames([{ id: "a", name: "A  B" }, { id: "b", name: "A B" }]);
    expect(new Set(labels.values()).size).toBe(2);
    expect(labels.get("a")).toBe("A B (1)");
  });
});
