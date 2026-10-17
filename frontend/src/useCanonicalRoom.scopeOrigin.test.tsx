import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useCanonicalRoom } from "./useCanonicalRoom";
import { closeRemoteWorkspace, installRemoteWorkspace } from "./lib/remote/remoteWorkspace";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";

describe("canonical room scope origin", () => {
  afterEach(() => {
    cleanup();
    closeRemoteWorkspace();
  });

  it("keeps the own room socket open across a member workspace install", async () => {
    closeRemoteWorkspace();
    const opened: unknown[] = [];
    const closed: unknown[] = [];
    const openSocket = vi.fn(() => {
      const handle = { close: () => { closed.push(handle); }, resync: () => undefined, ready: () => true, command: vi.fn(), historyBefore: vi.fn() };
      opened.push(handle);
      return handle;
    });
    const { rerender } = renderHook(() => useCanonicalRoom({
      roomId: "own-1",
      auth: { kind: "host", meetingId: "own-1" },
      serverSurface: TEST_SERVER_PRODUCT_SURFACE,
      openSocket: openSocket as never,
    }));
    await waitFor(() => expect(openSocket).toHaveBeenCalledOnce());

    // Installing a member host flips the global default workspace origin;
    // the own room's scope must stay on this computer's origin instead of
    // tearing its socket down and reopening it.
    act(() => {
      installRemoteWorkspace({
        transport: {
          hello: { origin: "https://hihi.test", server_id: "hihi-server", channel_id: "chan-1" },
          active: true, onClose: () => () => undefined, close: () => undefined,
        } as never,
        owner: null,
        member: { sessionToken: "member-token", serverSurface: { server_id: "hihi-server" } } as never,
        deviceToken: "device-test",
        clientId: "client-test",
      });
    });
    rerender();
    await act(async () => {});
    expect(openSocket).toHaveBeenCalledOnce();
    expect(closed).toHaveLength(0);
  });
});
