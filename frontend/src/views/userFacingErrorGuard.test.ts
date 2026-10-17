import { describe, expect, it } from "vitest";

// Deterministic lint guard: every rendered transport/host/server failure in
// views + app hooks must go through the single owner `userFacingError`.
// Raw `X instanceof Error ? X.message` ternaries leak WebKit/transport English
// ("Load failed"), browser text, or internal ids onto the screen.
//
// Allowed without the owner (non-render or explicitly disclosed):
// - useActiveMafiaGame: internal "Mafia game was not found"/404 control flow.
// - useRoomSettingsController / useMemberHosts return line: returns Error objects.
// - useMemberRoomLive category check: reads `category`, not `.message` render.
// - useAppController / useMemberRoomAutoReselect console.debug logging line (never rendered).
// - StartupIdentityGate `detail` behind the existing "자세히" disclosure and
//   its `"code" in reason` code checks.
const ALLOWLIST: Array<{ file: string; snippet: string }> = [
  { file: "useActiveMafiaGame.ts", snippet: "String(errorValue" },
  { file: "useMemberHosts.ts", snippet: "return reason instanceof Error ? reason :" },
  { file: "useMemberRoomLive.ts", snippet: '"category" in error' },
  { file: "useAppController.tsx", snippet: "message: error instanceof Error ? error.message : String(error)" },
  { file: "useMemberRoomAutoReselect.ts", snippet: "message: error instanceof Error ? error.message : String(error)" },
  { file: "useRoomSettingsController.ts", snippet: "return errorValue instanceof Error ? errorValue :" },
  { file: "StartupIdentityGate.tsx", snippet: "const detail = reason instanceof Error ? reason.message" },
  { file: "StartupIdentityGate.tsx", snippet: '"code" in reason' },
];

const viewModules = import.meta.glob("./**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const appModules = import.meta.glob("../app/**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

describe("userFacingError guard", () => {
  it("routes every rendered error.message through the single owner", () => {
    const entries = [...Object.entries(viewModules), ...Object.entries(appModules)].filter(
      ([path]) => !path.includes(".test."),
    );
    expect(entries.length).toBeGreaterThan(10);
    const violations: string[] = [];
    for (const [path, text] of entries) {
      const lines = String(text).split("\n");
      lines.forEach((line: string, index: number) => {
        if (!line.includes("instanceof Error") || !line.includes(".message")) return;
        if (line.includes("userFacingError")) return;
        if (ALLOWLIST.some(({ file: suffix, snippet }) => path.includes(suffix) && line.includes(snippet))) return;
        violations.push(`${path}:${index + 1}: ${line.trim().slice(0, 160)}`);
      });
    }
    expect(violations, violations.join("\n")).toEqual([]);
  });
});
