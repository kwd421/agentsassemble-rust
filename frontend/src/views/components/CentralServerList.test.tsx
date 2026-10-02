import { useState } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import CentralServerList from "./CentralServerList";
import { renameCentralServer, type CentralServer } from "../../lib/centralIdentity";

vi.mock("../../lib/centralIdentity", () => ({ renameCentralServer: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const host: CentralServer = { server_id: "server-0001", alias: "Mac Studio", relation: "owner", endpoint: null, host_public_key_jwk: {}, host_key_fingerprint: "test" };

it("allows offline owner rename and displays the refreshed central name", async () => {
  const user = userEvent.setup();
  vi.mocked(renameCentralServer).mockResolvedValue();
  function Harness() {
    const [servers, setServers] = useState([host]);
    return <CentralServerList servers={servers} busy={false} onOpen={async () => { throw new Error("offline host must not open"); }} onRefresh={async () => { setServers([{ ...host, alias: "내 서버" }]); }} />;
  }
  render(<Harness />);
  expect((screen.getByRole("button", { name: "Mac Studio 서버 열기" }) as HTMLButtonElement).disabled).toBe(true);
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
  render(<CentralServerList servers={[host, { ...host, server_id: "bookmark-0002", relation: "bookmark", alias: "Friend" }]} busy={false} onOpen={async () => {}} onRefresh={async () => {}} />);
  expect(screen.queryByRole("button", { name: "Friend 이름 변경" })).toBeNull();
  await user.click(screen.getByRole("button", { name: /이름 변경/ }));
  await user.clear(screen.getByRole("textbox"));
  await user.type(screen.getByRole("textbox"), "보존할 입력");
  await user.click(screen.getByRole("button", { name: "이름 저장" }));
  expect((await screen.findByRole("alert")).textContent).toContain("목록이 바뀌었습니다");
  expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("보존할 입력");
});
