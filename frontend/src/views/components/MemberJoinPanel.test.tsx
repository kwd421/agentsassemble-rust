// @vitest-environment-options {"url":"https://host.test/"}
import { StrictMode } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MemberJoinPanel from "./MemberJoinPanel";
import { ApiError } from "../../lib/apiErrors";
import { consumeCentralMemberRequest, consumeMemberReturn, createMemberHandoff, memberTargetRequest, storeMemberHandoff } from "../../lib/central/memberConnect";

const mocks = vi.hoisted(() => ({
  challenge: vi.fn(), join: vi.fn(), preview: vi.fn(), issue: vi.fn(), session: vi.fn(), nativeLogin: vi.fn(),
  desktop: vi.fn(), webLogin: vi.fn(), webReturn: vi.fn(), navigate: vi.fn(),
}));
vi.mock("../../api/invites", () => ({ challengeRoomMember: mocks.challenge, joinRoomMember: mocks.join }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: mocks.desktop }));
vi.mock("../../lib/central/identity", () => ({
  centralAccountEntryUrl: () => "https://central.test/", loadCentralSession: mocks.session,
  issueCentralMemberGrant: mocks.issue, previewCentralMember: mocks.preview, loginCentralGoogle: mocks.nativeLogin,
}));
vi.mock("../../lib/central/webGoogle", () => ({ startCentralWebGoogle: mocks.webLogin, completeCentralWebGoogleReturn: mocks.webReturn }));

const challenge = { server_id: "server", registration_epoch: "epoch", challenge_id: "challenge",
  challenge_hash: "a".repeat(43), expires_at: Math.floor(Date.now() / 1000) + 300 };
const target = { server_id: "server", label: "친구의 서버", endpoint_origin: "https://host.test", endpoint_generation: 7 };
const grant = { ...target, label: undefined, registration_epoch: "epoch", grant_token: `aamg1.${"g".repeat(43)}`, expires_at: challenge.expires_at };
const session = { token: "central-private-session", person: { display_name: "중앙 사용자", person_id: "person" } };
const host = () => ({ inviteToken: "private-invite", meetingId: "room", deviceToken: "browser-credential", clientId: "client",
  onComplete: vi.fn().mockResolvedValue(true) });
const realWindow = window;

beforeEach(() => {
  vi.clearAllMocks(); realWindow.sessionStorage.clear(); realWindow.history.replaceState({}, "", "/join?token=private-invite");
  mocks.desktop.mockReturnValue(true); mocks.session.mockReturnValue(session);
  mocks.challenge.mockResolvedValue(challenge); mocks.preview.mockResolvedValue(target);
  const { label: _, ...issued } = grant;
  mocks.issue.mockResolvedValue(issued); mocks.join.mockResolvedValue({ server_id: "server", session_token: "host-session" });
  mocks.webReturn.mockResolvedValue(undefined);
  vi.stubGlobal("window", new Proxy(realWindow, {
    get(object, property) {
      if (property === "location") return { origin: realWindow.location.origin, pathname: realWindow.location.pathname,
        hash: realWindow.location.hash, search: realWindow.location.search, assign: mocks.navigate };
      return Reflect.get(object, property, object);
    },
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

async function agree() {
  fireEvent.click(await screen.findByRole("button", { name: "동의하고 입장" }));
}

describe("member invite screen", () => {
  it("requires explicit consent, displays canonical account/server/origin, and admits natively once", async () => {
    const props = host(); render(<StrictMode><MemberJoinPanel host={props} /></StrictMode>);
    await screen.findByText("중앙 사용자");
    expect(screen.getByText("친구의 서버 (server)")).toBeTruthy();
    expect(screen.getByText("https://host.test")).toBeTruthy();
    expect(mocks.issue).not.toHaveBeenCalled(); expect(mocks.join).not.toHaveBeenCalled();
    await agree(); await waitFor(() => expect(props.onComplete).toHaveBeenCalledOnce());
    expect(mocks.challenge).toHaveBeenCalledOnce(); expect(mocks.issue).toHaveBeenCalledOnce();
    expect(mocks.join).toHaveBeenCalledWith(expect.objectContaining({ challenge_id: "challenge", invite_token: "private-invite" }),
      grant.grant_token, expect.any(String), "client", "browser-credential");
    expect(sessionStorage.length).toBe(0); expect(mocks.navigate).not.toHaveBeenCalled();
  });

  it("reuses native login, then stops at consent", async () => {
    mocks.session.mockReturnValue(null);
    mocks.nativeLogin.mockImplementation(async () => { mocks.session.mockReturnValue(session); return session; });
    render(<MemberJoinPanel host={host()} />);
    await screen.findByRole("button", { name: "동의하고 입장" });
    expect(mocks.nativeLogin).toHaveBeenCalledOnce(); expect(mocks.issue).not.toHaveBeenCalled();
  });

  it.each([
    ["host", "서버에 연결할 수 없어요", new TypeError("fetch failed")],
    ["central", "중앙 서버에 연결할 수 없어요", new Error("중앙 서버에 연결할 수 없어요")],
    ["expiry", "만료", new ApiError(401, "private internal failure", "member_challenge_invalid")],
    ["redeem", "중앙 계정을 확인하지 못했어요", new ApiError(502, "internal", "member_redeem_failed")],
    ["left/kicked", "나갔거나 강퇴된 방", new ApiError(403, "internal", "admission_session_unavailable")],
    ["other invite", "이미 다른 초대", new ApiError(409, "internal", "idempotency_conflict")],
  ])("shows %s failure and retries from a fresh challenge without login", async (stage, message, error) => {
    if (stage === "host") mocks.challenge.mockRejectedValueOnce(error);
    else if (stage === "central") mocks.preview.mockRejectedValueOnce(error);
    else mocks.join.mockRejectedValueOnce(error);
    render(<MemberJoinPanel host={host()} />);
    if (stage !== "host" && stage !== "central") await agree();
    expect((await screen.findByRole("alert")).textContent).toContain(message);
    const before = mocks.challenge.mock.calls.length;
    await Promise.resolve(); expect(mocks.challenge).toHaveBeenCalledTimes(before);
    mocks.challenge.mockResolvedValue({ ...challenge, challenge_id: "fresh", challenge_hash: "b".repeat(43) });
    fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    await screen.findByRole("button", { name: "동의하고 입장" });
    expect(mocks.challenge).toHaveBeenCalledTimes(before + 1);
    expect(mocks.nativeLogin).not.toHaveBeenCalled();
    await agree(); await waitFor(() => expect(mocks.join).toHaveBeenLastCalledWith(
      expect.objectContaining({ challenge_id: "fresh" }), grant.grant_token, expect.any(String), "client", "browser-credential"));
  });

  it.each([{ endpoint_origin: "https://changed.test" }, { endpoint_generation: 8 }])("refuses a changed consent target %j", async (change) => {
    mocks.issue.mockResolvedValue({ ...grant, ...change });
    render(<MemberJoinPanel host={host()} />); await agree();
    expect((await screen.findByRole("alert")).textContent).toContain("서버 주소가 변경");
    expect(mocks.join).not.toHaveBeenCalled(); expect(mocks.navigate).not.toHaveBeenCalled();
  });

  it("refuses an account switch before the consent click", async () => {
    render(<MemberJoinPanel host={host()} />);
    await screen.findByText("중앙 사용자"); mocks.session.mockReturnValue({ ...session, token: "different" });
    await agree(); expect((await screen.findByRole("alert")).textContent).toContain("계정이 바뀌");
    expect(mocks.issue).not.toHaveBeenCalled();
  });

  it("round-trips host storage and central login, grants only on consent, consumes callback before join", async () => {
    mocks.desktop.mockReturnValue(false);
    const props = host(); const first = render(<MemberJoinPanel host={props} />);
    await waitFor(() => expect(mocks.navigate).toHaveBeenCalledOnce());
    const destination = new URL(mocks.navigate.mock.calls[0][0]);
    expect(destination.origin).toBe("https://central.test"); expect(destination.pathname).toBe("/member-join");
    expect(mocks.preview).not.toHaveBeenCalled(); expect(mocks.issue).not.toHaveBeenCalled();
    const hostStorage = Object.fromEntries(Object.keys(sessionStorage).map(key => [key, sessionStorage.getItem(key)!]));
    first.unmount(); sessionStorage.clear();
    realWindow.history.replaceState({}, "", `/member-join${destination.hash}`);
    const request = consumeCentralMemberRequest()!;
    expect(Object.keys(request).sort()).toEqual(["challenge_hash", "handoff_state", "registration_epoch", "server_id"]);
    mocks.session.mockReturnValue(null);
    const central = render(<MemberJoinPanel request={request} />);
    fireEvent.click(await screen.findByRole("button", { name: "Google로 계속" }));
    await waitFor(() => expect(mocks.webLogin).toHaveBeenCalledOnce());
    expect(mocks.issue).not.toHaveBeenCalled(); central.unmount();
    realWindow.history.replaceState({}, "", "/?code=test-code&state=test-state");
    mocks.webReturn.mockImplementationOnce(async () => { mocks.session.mockReturnValue(session); });
    const returned = render(<MemberJoinPanel request={consumeCentralMemberRequest()} />);
    await screen.findByRole("button", { name: "동의하고 입장" }); expect(mocks.issue).not.toHaveBeenCalled();
    await agree(); await waitFor(() => expect(mocks.navigate).toHaveBeenCalledTimes(2));
    expect(sessionStorage.length).toBe(0); returned.unmount();
    Object.entries(hostStorage).forEach(([key, value]) => sessionStorage.setItem(key, value));
    const callbackUrl = new URL(mocks.navigate.mock.calls[1][0]);
    realWindow.history.replaceState({}, "", `${callbackUrl.pathname}${callbackUrl.hash}`);
    const callback = consumeMemberReturn()!;
    expect(realWindow.location.hash).toBe(""); expect(sessionStorage.length).toBe(0);
    expect(callback.error).toBeUndefined();
    render(<StrictMode><MemberJoinPanel host={{ ...props, callback }} /></StrictMode>);
    await waitFor(() => expect(props.onComplete).toHaveBeenCalledOnce());
    expect(mocks.join).toHaveBeenCalledOnce(); expect(mocks.challenge).toHaveBeenCalledOnce();
  });

  it("displays an invalid callback and waits for explicit fresh challenge retry", async () => {
    render(<MemberJoinPanel host={{ ...host(), callback: { record: createMemberHandoff(challenge, "private-invite", "room"), error: "입장 요청이 만료됐어요." } }} />);
    await screen.findByRole("alert"); expect(mocks.challenge).not.toHaveBeenCalled(); expect(mocks.join).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    await screen.findByRole("button", { name: "동의하고 입장" }); expect(mocks.challenge).toHaveBeenCalledOnce();
  });

  it("does not redirect or issue on central entry before the user consents", async () => {
    render(<MemberJoinPanel request={memberTargetRequest(createMemberHandoff(challenge, "invite", "room"))} />);
    expect(screen.queryByRole("button", { name: "Google로 계속" })).toBeNull();
    await screen.findByRole("button", { name: "동의하고 입장" });
    expect(mocks.issue).not.toHaveBeenCalled(); expect(mocks.navigate).not.toHaveBeenCalled();
  });

  it("returns an explicit central retry without a grant, then starts a new host challenge", async () => {
    const record = createMemberHandoff(challenge, "private-invite", "room");
    storeMemberHandoff(record);
    mocks.issue.mockRejectedValueOnce(new Error("중앙 서버에 연결할 수 없어요."));
    const central = render(<MemberJoinPanel request={memberTargetRequest(record)} />);
    await agree(); await screen.findByRole("alert");
    expect(mocks.navigate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
    const url = new URL(mocks.navigate.mock.calls[0][0]);
    realWindow.history.replaceState({}, "", `/join${url.hash}`);
    const callback = consumeMemberReturn()!;
    expect(callback.retry).toBe(true); expect(callback.grant).toBeUndefined();
    central.unmount();
    render(<MemberJoinPanel host={{ ...host(), callback }} />);
    await screen.findByRole("button", { name: "동의하고 입장" });
    expect(mocks.challenge).toHaveBeenCalledOnce(); expect(mocks.join).not.toHaveBeenCalled();
    expect(mocks.issue).toHaveBeenCalledOnce();
  });
});
