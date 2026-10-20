import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import MemberJoinPanel from "./MemberJoinPanel";

const admission = vi.hoisted(() => ({
  admit: vi.fn(), select: vi.fn(), close: vi.fn(),
}));
const secure = vi.hoisted(() => ({
  open: vi.fn(),
  preview: vi.fn(),
  session: null as null | { token: string; person: { display_name: string; person_id: string } },
}));
vi.mock("../../lib/central/secureMemberEntry", () => ({
  previewSecureMember: (...args: unknown[]) => secure.preview(...args),
  SecureMemberAdmission: { open: (...args: unknown[]) => secure.open(...args) },
}));
vi.mock("../../lib/central/identity", () => ({
  loadCentralSession: () => secure.session,
  loadPendingCentralRecoveryCode: () => "",
  setCentralMemberHidden: vi.fn(),
}));

const target = {
  server_id: "friend", registration_epoch: "epoch", origin: "https://friend.test", generation: 3,
  host_public_key_jwk: {}, host_key_fingerprint: "fingerprint",
};
const entry = { server_id: "friend", registration_epoch: "epoch" };

describe("secure first-join consent", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    window.sessionStorage.clear();
    secure.session = { token: "central-token", person: { display_name: "나", person_id: "person" } };
    secure.preview.mockResolvedValue({
      target, label: "친구의 서버", endpoint_origin: target.origin, endpoint_generation: 3, server_id: "friend",
    });
    secure.open.mockResolvedValue(admission);
    admission.admit.mockResolvedValue([{ room_id: "general", name: "일반" }, { room_id: "random", name: "" }]);
    admission.select.mockResolvedValue({});
  });
  afterEach(cleanup);

  it("writes only the display anchor after in-place consent and room choice", async () => {
    const href = window.location.href;
    render(<StrictMode><MemberJoinPanel secureEntry={entry} onCancel={() => undefined} /></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "다시 연결" }));
    fireEvent.click(await screen.findByRole("button", { name: "일반" }));
    await vi.waitFor(() => {
      const raw = window.localStorage.getItem("agentsassemble.memberAnchors.v1:person");
      expect(raw).toBeTruthy();
    });
    const stored = JSON.parse(window.localStorage.getItem("agentsassemble.memberAnchors.v1:person") || "{}");
    expect(stored).toEqual({ anchors: [{
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 3,
      rooms: [{ room_id: "general", name: "일반" }, { room_id: "random", name: "" }],
      hidden: false, updated_at: expect.any(Number),
    }] });
    expect(JSON.stringify(stored)).not.toMatch(/grant|session_token|private|aad1_/i);
    expect(window.location.href).toBe(href);
    expect(admission.select).toHaveBeenCalledWith("general");
  });

  it("offers the current name on an invite join and sends the edited one", async () => {
    render(<MemberJoinPanel secureEntry={{ ...entry, inviteToken: "aaj1.invite" }} onCancel={() => undefined} />);
    const name = await screen.findByRole("textbox", { name: "이름" }) as HTMLInputElement;
    expect(name.value).toBe("나");
    fireEvent.change(name, { target: { value: "  방 이름  " } });
    fireEvent.click(screen.getByRole("button", { name: "참가하기" }));
    await vi.waitFor(() => expect(admission.admit).toHaveBeenCalled());
    expect(admission.admit.mock.calls[0][1]).toEqual({ display_name: "방 이름" });
  });

  it("does not offer profile fields when reconnecting", async () => {
    render(<MemberJoinPanel secureEntry={entry} onCancel={() => undefined} />);
    await screen.findByRole("button", { name: "다시 연결" });
    expect(screen.queryByRole("textbox", { name: "이름" })).toBeNull();
  });
});
