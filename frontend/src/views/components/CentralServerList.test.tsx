import { useState } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import CentralServerList from "./CentralServerList";
import "../../test/nativeDialog";
import { loadRoomDockItems } from "../../lib/roomDockPersistence";
vi.mock("../../lib/roomDockPersistence", () => ({ loadRoomDockItems: vi.fn(() => []) }));
import { fetchCentralServerIcon, renameCentralServer, setCentralServerIcon, type CentralServer } from "../../lib/central/identity";

vi.mock("../../lib/central/identity", () => ({
  renameCentralServer: vi.fn(),
  setCentralServerIcon: vi.fn(),
  fetchCentralServerIcon: vi.fn(),
}));
vi.mock("./ImageCropper", () => ({
  default: ({ file, onCropped }: { file: File; onCropped: (file: File) => void }) => (
    <button type="button" onClick={() => onCropped(file)}>적용</button>
  ),
}));
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.mocked(loadRoomDockItems).mockReturnValue([]); });
const host: CentralServer = { server_id: "server-0001", alias: "Mac Studio", host_os: "macos", relation: "owner", endpoint: null, host_public_key_jwk: {}, host_key_fingerprint: "test" };

it("matches the local installation by ID, opens it explicitly and keeps rename", async () => {
  const user = userEvent.setup();
  vi.mocked(renameCentralServer).mockResolvedValue();
  const openLocal = vi.fn().mockResolvedValue(undefined);
  function Harness() {
    const [servers, setServers] = useState([host]);
    return <CentralServerList servers={servers} liveServers={servers} busy={false} localHost={{ server_id: host.server_id, host_name: "Different computer name", host_os: "macos", device_kind: "Mac Studio", profile_name: "Owner" }} onOpenLocal={openLocal} onOpen={async () => { throw new Error("offline host must not open"); }} onRefresh={async () => { setServers([{ ...host, alias: "내 서버" }]); }} />;
  }
  render(<Harness />);
  expect((screen.getByRole("button", { name: "Mac Studio 서버 열기" }) as HTMLButtonElement).disabled).toBe(false);
  expect(screen.getByText(/이 기기/, { selector: "strong" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "이 기기 서버 열기" })).toBeNull();
  expect(openLocal).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Mac Studio 서버 열기" }));
  expect(openLocal).toHaveBeenCalledOnce();
  await user.click(screen.getByRole("button", { name: /이름 변경/ }));
  await user.clear(screen.getByRole("textbox", { name: "서버 이름" }));
  await user.type(screen.getByRole("textbox", { name: "서버 이름" }), "내 서버");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  expect(await screen.findByRole("button", { name: "내 서버 이름 변경" })).toBeTruthy();
  expect(screen.queryByRole("textbox")).toBeNull();
});

it("retains failed edits and excludes bookmarks from owner controls", async () => {
  const user = userEvent.setup();
  vi.mocked(renameCentralServer).mockRejectedValue(new Error("목록이 바뀌었습니다"));
  render(<CentralServerList servers={[host, { ...host, server_id: "bookmark-0002", relation: "bookmark", alias: "Friend" }]} liveServers={[host]} busy={false} onOpen={async () => {}} onRefresh={async () => {}} />);
  expect((screen.getByRole("button", { name: "Mac Studio 서버 열기" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "Friend 이름 변경" })).toBeNull();
  await user.click(screen.getByRole("button", { name: /이름 변경/ }));
  await user.clear(screen.getByRole("textbox"));
  await user.type(screen.getByRole("textbox"), "보존할 입력");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  expect((await screen.findByRole("alert")).textContent).toContain("목록이 바뀌었습니다");
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("보존할 입력");
});

it("uses the refreshed name when retrying a conflicting edit", async () => {
  const user = userEvent.setup();
  let stored = "Other device's new name";
  vi.mocked(renameCentralServer).mockImplementation(async (server, name) => {
    if (server.alias !== stored) throw new Error("목록이 바뀌었습니다");
    stored = name === null ? "Mac Studio" : name;
  });
  function Harness() {
    const [servers, setServers] = useState([host]);
    const refresh = async () => { setServers([{ ...host, alias: stored }]); };
    return <><CentralServerList servers={servers} liveServers={servers} busy={false} onOpen={async () => {}} onRefresh={refresh} />
      <button onClick={() => void refresh()}>새로고침</button></>;
  }
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: /이름 변경/ }));
  await user.clear(screen.getByRole("textbox"));
  await user.type(screen.getByRole("textbox"), "My chosen name");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  await screen.findByRole("alert");
  await user.click(screen.getByRole("button", { name: "새로고침" }));
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("My chosen name");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  expect(await screen.findByRole("button", { name: "My chosen name 이름 변경" })).toBeTruthy();
});

it("lets only the owner replace a server icon with the cropped image and then reloads the list", async () => {
  const user = userEvent.setup();
  vi.mocked(fetchCentralServerIcon).mockReturnValue(new Promise(() => {}));
  vi.mocked(setCentralServerIcon).mockResolvedValue("/v1/servers/server-0001/icon/abc.png");
  const onRefresh = vi.fn().mockResolvedValue(undefined);
  render(<CentralServerList servers={[{ ...host, icon: "/v1/servers/server-0001/icon/old.png" }, { ...host, server_id: "bookmark-0002", relation: "bookmark", alias: "Friend" }]}
    liveServers={[host]} busy={false} onOpen={async () => {}} onRefresh={onRefresh} />);
  expect(screen.queryByRole("button", { name: "Friend 아이콘 변경" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Mac Studio 아이콘 변경" }));
  const picked = new File(["png"], "icon.png", { type: "image/png" });
  await user.upload(screen.getByLabelText("서버 아이콘 이미지 선택"), picked);
  await user.click(screen.getByRole("button", { name: "적용" }));
  expect(setCentralServerIcon).toHaveBeenCalledWith(expect.objectContaining({ server_id: "server-0001", icon: "/v1/servers/server-0001/icon/old.png" }), picked);
  expect(onRefresh).toHaveBeenCalledOnce();
});

it("dims central-dependent cached servers while keeping this-device open available", () => {
  render(<CentralServerList servers={[host, { ...host, server_id: "remote", alias: "Other Mac" }]}
    liveServers={[]} centralUnavailable busy={false}
    localHost={{ server_id: host.server_id, host_name: "Mac", host_os: "macos", device_kind: "Mac Studio", profile_name: "Owner" }}
    onOpenLocal={async () => {}} onOpen={async () => {}} onRefresh={async () => {}} />);
  expect(screen.getByRole("button", { name: "Mac Studio 서버 열기" })).toHaveProperty("disabled", false);
  expect(screen.getByRole("button", { name: "Other Mac 서버 열기" })).toHaveProperty("disabled", true);
  expect(screen.getByText(/연결 끊김 · 로그인 서버 확인 불가/).closest(".dc-server-row")?.getAttribute("data-state")).toBe("central-unconfirmed");
});

it("does not expose full or shortened server IDs in the saved chooser", () => {
  const server = { ...host, server_id: "12345678-1234-4234-8234-123456789abc", alias: "" };
  render(<CentralServerList servers={[server]} liveServers={[]} busy={false} onOpen={async () => {}} onRefresh={async () => {}} />);
  expect(screen.getByText("이름 없는 서버")).toBeTruthy();
  expect(document.body.textContent).not.toContain(server.server_id.slice(0, 8));
  expect(screen.getByRole("button", { name: "이름 없는 서버 서버 열기" })).toBeTruthy();
});


it("edits the first host name before opening and can return to the profile default", async () => {
  const user = userEvent.setup();
  const opened: Array<string | undefined> = [];
  render(<CentralServerList servers={[]} liveServers={[]} busy={false} profileName="Account Name"
    localHost={{ server_id: null, host_name: "Private hostname", host_os: "macos", device_kind: "MacBook Air", profile_name: "Edited Profile" }}
    onOpenLocal={async name => { opened.push(name); }} onOpen={async () => {}} onRefresh={async () => {}} />);
  expect(document.body.textContent).toContain("Edited Profile의 MacBook Air");
  expect(document.body.textContent).not.toContain("Private hostname");
  await user.click(screen.getByRole("button", { name: "편집" }));
  await user.clear(screen.getByRole("textbox", { name: "서버 이름" }));
  await user.type(screen.getByRole("textbox", { name: "서버 이름" }), "작업용 컴퓨터");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  await user.click(screen.getByRole("button", { name: "이 기기 서버 열기" }));
  expect(opened).toEqual(["작업용 컴퓨터"]);
  await user.click(screen.getByRole("button", { name: "편집" }));
  await user.click(screen.getByRole("button", { name: "기본 이름으로 되돌리기" }));
  await user.click(screen.getByRole("button", { name: "이 기기 서버 열기" }));
  expect(opened).toEqual(["작업용 컴퓨터", undefined]);
  expect(document.body.textContent).toContain("Edited Profile의 MacBook Air");
});

it("restores a fixed server name only after the existing rename operation succeeds", async () => {
  const user = userEvent.setup();
  const initial = { ...host, alias: "작업용", default_name: "Owner의 Mac Studio", name_is_default: false };
  let saved = initial;
  vi.mocked(renameCentralServer).mockRejectedValueOnce(new Error("저장 실패"))
    .mockImplementationOnce(async (_server, name) => {
      if (name !== null) throw new Error("Reset must restore automatic naming");
      saved = { ...initial, alias: initial.default_name, name_is_default: true };
    });
  function Harness() {
    const [server, setServer] = useState(initial);
    return <CentralServerList servers={[server]} liveServers={[server]} busy={false} onOpen={async () => {}}
      onRefresh={async () => { setServer(saved); }} />;
  }
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "작업용 이름 변경" }));
  await user.click(screen.getByRole("button", { name: "기본 이름으로 되돌리기" }));
  await screen.findByRole("alert");
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("작업용");
  await user.click(screen.getByRole("button", { name: "기본 이름으로 되돌리기" }));
  expect(await screen.findByRole("button", { name: "Owner의 Mac Studio 이름 변경" })).toBeTruthy();
});

 it("previews the first room belonging to the edited server, or a new room", async () => {
  const user = userEvent.setup();
  vi.mocked(loadRoomDockItems).mockReturnValue([
    { serverId: "other", label: "다른 서버 방" },
    { serverId: host.server_id, label: "실제 첫 방" },
    { serverId: host.server_id, label: "두 번째 방" },
  ] as ReturnType<typeof loadRoomDockItems>);
  render(<CentralServerList servers={[host]} liveServers={[]} busy={false} onOpen={async () => {}} onRefresh={async () => {}} />);
  await user.click(screen.getByRole("button", { name: "Mac Studio 이름 변경" }));
  expect(screen.getByText(/‘실제 첫 방’ · Mac Studio에서 열린 방/)).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "취소" }));
  vi.mocked(loadRoomDockItems).mockReturnValue([]);
  await user.click(screen.getByRole("button", { name: "Mac Studio 이름 변경" }));
  expect(screen.getByText(/‘새 회의실’ · Mac Studio에서 열린 방/)).toBeTruthy();
});
