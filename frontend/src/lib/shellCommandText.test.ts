import { describe, expect, it } from "vitest";
import { shellCommandText } from "./shellCommandText";

describe("shellCommandText", () => {
  it("keeps a path with spaces as one pasted argument", () => {
    expect(
      shellCommandText(["npm", "install", "--global", "--prefix", "C:\Users\Jane Doe\npm", "opencode-ai@1.4.2"])
    ).toBe('npm install --global --prefix "C:\Users\Jane Doe\npm" opencode-ai@1.4.2');
  });

  it("leaves plain arguments unquoted and escapes an embedded quote", () => {
    expect(shellCommandText(["npm", "install"])).toBe("npm install");
    expect(shellCommandText(['a"b'])).toBe('"a`"b"');
  });
});
