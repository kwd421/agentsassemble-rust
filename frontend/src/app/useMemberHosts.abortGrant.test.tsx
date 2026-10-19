import { cleanup, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberHosts } from "./useMemberHosts";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import type { CentralDirectoryState } from "./useCentralDirectory";

const counters = vi.hoisted(() => ({ previews: 0, opens: 0, grants: 0 }));

vi.mock("../lib/central/memberHostOriginProbe", () => ({
  probeMemberHostOrigin: vi.fn(
    () => new Promise((resolve) => setTimeout(() => resolve(null), 5)),
  ),
}));

vi.mock("../lib/central/secureMemberEntry", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../lib/central/secureMemberEntry")>();
  class FakeAdmission {
    transport = {
      active: true,
      onClose: vi.fn(),
      close: vi.fn(),
      hello: { origin: "https://friend.test" },
    };
    close = vi.fn();
    select = vi.fn();
    async admit(_signal?: AbortSignal) {
      counters.grants += 1;
      return [{ room_id: "general", name: "일반" }];
    }
  }
  return {
    ...actual,
    previewSecureMember: vi.fn(async () => {
      counters.previews += 1;
      await new Promise((resolve) => setTimeout(resolve, 20));
      return {
        target: {
          server_id: "friend",
          registration_epoch: "epoch",
          origin: "https://friend.test",
          generation: 3,
          host_key_fingerprint: "fingerprint",
          host_public_key_jwk: {},
        },
        label: "친구",
        endpoint_origin: "https://friend.test",
        endpoint_generation: 3,
        server_id: "friend",
      };
    }),
    SecureMemberAdmission: {
      open: vi.fn(async () => {
        counters.opens += 1;
        return new FakeAdmission();
      }),
    },
  };
});

const liveServer = () => ({
  server_id: "friend",
  registration_epoch: "epoch",
  relation: "member" as const,
  alias: "친구",
  host_key_fingerprint: "fingerprint",
  host_public_key_jwk: {},
  endpoint: {
    origin: "https://friend.test",
    generation: 3,
    status: "published" as const,
    mode: "event_secure_v1",
    protocol: "secure_admission_v1",
  },
});

const directoryFor = (): CentralDirectoryState => ({
  status: "connected" as const,
  person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
  servers: [],
  live: {
    person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
    servers: [liveServer()],
    server_time: 1,
  },
  error: undefined,
});

describe("useMemberHosts startup abort", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    counters.previews = 0;
    counters.opens = 0;
    counters.grants = 0;
    vi.clearAllMocks();
  });

  it("mount -> immediate identity flap spends exactly one connect grant", async () => {
    // StrictMode-like: the first reconcile's attach flight is aborted by the
    // identityKey effect cleanup (dev double-effects, or a quick identity
    // flap in production) while its attach still runs. The aborted flight
    // must not spend central after abort and must never delete or overwrite
    // the newer flight's bookkeeping.
    const directory = directoryFor();
    // A saved anchor forces the attach through the async probe first, so the
    // flap below aborts the first flight before its central preview is sent.
    saveMemberAnchor("person", {
      server_id: "friend",
      registration_epoch: "epoch",
      host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test",
      endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }],
      hidden: false,
      updated_at: 1,
    });
    const { result, rerender } = renderHook(
      ({ sessionToken }: { sessionToken: string }) =>
        useMemberHosts({ personId: "person", sessionToken, directory }),
      { initialProps: { sessionToken: "token-1" } },
    );
    rerender({ sessionToken: "token-2" });
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    // Exactly one preview, one transport open, one connect grant and one
    // live admission: the aborted flight spends nothing after abort.
    expect(counters.previews).toBe(1);
    expect(counters.opens).toBe(1);
    expect(counters.grants).toBe(1);
    expect(result.current.snapshots).toHaveLength(1);
    expect(result.current.snapshots[0]).toMatchObject({ serverId: "friend", phase: "connected" });
  });
});
