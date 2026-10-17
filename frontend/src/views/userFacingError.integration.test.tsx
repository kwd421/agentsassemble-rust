import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import CentralServerList from "./components/CentralServerList";
import { uploadFailureMessage } from "./lobby/useOutgoingMessages";
import { ApiError } from "../lib/apiErrors";
import "../test/nativeDialog";
import type { CentralServer } from "../lib/central/identity";

vi.mock("../lib/roomDockPersistence", () => ({ loadRoomDockItems: vi.fn(() => []) }));
vi.mock("../lib/central/identity", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/central/identity")>();
  return {
    ...actual,
    hasPendingLocalDemotion: vi.fn(() => false),
    retryPendingLocalDemotion: vi.fn(),
    resolveCentralServerDuplicates: vi.fn(),
    renameCentralServer: vi.fn(),
    setCentralServerIcon: vi.fn(),
    fetchCentralServerIcon: vi.fn(),
  };
});
import { renameCentralServer } from "../lib/central/identity";

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

const host: CentralServer = {
  server_id: "server-0001",
  alias: "Mac Studio",
  host_os: "macos",
  relation: "owner",
  endpoint: null,
  host_public_key_jwk: {},
  host_key_fingerprint: "test",
};

describe("userFacingError integration", () => {
  it("maps upload transport failures to the neutral Korean fallback", () => {
    const message = uploadFailureMessage(new TypeError("Load failed"));
    expect(message).toBe("파일을 올리지 못했어요.");
    expect(message).not.toContain("Load failed");
  });

  it("shows the neutral Korean fallback for a WebKit fetch failure on rename", async () => {
    const user = userEvent.setup();
    vi.mocked(renameCentralServer).mockRejectedValue(new TypeError("Load failed"));
    render(
      <CentralServerList
        servers={[host]}
        liveServers={[host]}
        busy={false}
        onOpen={async () => {}}
        onRefresh={async () => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: /이름 변경/ }));
    await user.clear(screen.getByRole("textbox", { name: "서버 이름" }));
    await user.type(screen.getByRole("textbox", { name: "서버 이름" }), "새 이름");
    await user.click(screen.getByRole("button", { name: "이름 저장" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("서버 이름을 저장하지 못했어요.");
    expect(alert.textContent).not.toContain("Load failed");
  });

  it("maps a known server code to its Korean text on rename", async () => {
    const user = userEvent.setup();
    vi.mocked(renameCentralServer).mockRejectedValue(
      new ApiError(409, "conflict", "duplicate_resolution_conflict"),
    );
    render(
      <CentralServerList
        servers={[host]}
        liveServers={[host]}
        busy={false}
        onOpen={async () => {}}
        onRefresh={async () => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: /이름 변경/ }));
    await user.clear(screen.getByRole("textbox", { name: "서버 이름" }));
    await user.type(screen.getByRole("textbox", { name: "서버 이름" }), "새 이름");
    await user.click(screen.getByRole("button", { name: "이름 저장" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("다른 기기에서 서버를 선택했어요. 목록을 다시 확인해 주세요.");
  });
});
