import { describe, expect, it } from "vitest";

import { canonicalRoomId } from "./canonicalRoomId";

describe("canonicalRoomId", () => {
  it("matches Rust whitespace instead of ECMAScript trim semantics", () => {
    expect(canonicalRoomId("\ufeffroom")).toBe("\ufeffroom");
    expect(() => canonicalRoomId("\u0085room")).toThrow(/정규 형식/);
  });

  it("counts Unicode scalars and rejects values Rust strings cannot represent", () => {
    expect(canonicalRoomId("\u{10000}".repeat(128))).toBe("\u{10000}".repeat(128));
    expect(() => canonicalRoomId("\u{10000}".repeat(129))).toThrow(/정규 형식/);
    expect(() => canonicalRoomId("\ud800room")).toThrow(/정규 형식/);
  });
});
