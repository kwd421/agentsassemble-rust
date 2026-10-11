import {act,cleanup,fireEvent,render,screen} from "@testing-library/react";
import {afterEach,expect,it,vi} from "vitest";
import ServerBackupCard from "./ServerBackupCard";
const native=vi.hoisted(()=>vi.fn());
vi.mock("../../lib/desktopBridge",()=>({
  isDesktopWebview:()=>true,
  requestDesktopHostDeviceInfo:async()=>({server_id:"server"}),
  requestDesktopServerBackup:native,
}));
vi.mock("../../lib/central/identity",()=>({
  bootstrapCentral:async()=>({servers:[{server_id:"server",relation:"owner",alias:"Owner server"}]}),
  loadCentralSession:()=>({person:{person_id:"owner"}}),
}));
afterEach(()=>{cleanup();vi.useRealTimers();native.mockReset();});
// A lost status response must not strand a recovered running export after retry.
it("recovers the runtime job on remount and restarts status observation after transport failure",async()=>{
  vi.useFakeTimers();
  const progress={operation_id:"abcdefab-1234-1234-1234-123456789abc",phase:"encoding",snapshot_at:null,bytes:null};
  native.mockResolvedValueOnce(progress).mockRejectedValueOnce(new Error("connection lost"))
    .mockResolvedValueOnce(progress).mockResolvedValueOnce({...progress,phase:"complete",snapshot_at:"2026-10-11T00:00:00Z",bytes:1024});
  await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
  expect(screen.getByRole("status").textContent).toContain("백업 파일을 만들고 있어요");
  await act(async()=>{await vi.advanceTimersByTimeAsync(1000);});
  expect(screen.getByRole("alert").textContent).toContain("connection lost");
  await act(async()=>{fireEvent.click(screen.getByText("다시 확인"));});
  await act(async()=>{await vi.advanceTimersByTimeAsync(1000);});
  expect(screen.getByRole("status").textContent).toContain("저장했어요");
  expect(native.mock.calls.map(([request])=>request.action)).toEqual(["latest","status","latest","status"]);
  expect(screen.queryByText("취소")).toBeNull();
});

it("shows loading until backup status arrives, then explains a bounded failure",async()=>{
  let resolve!:(value:unknown)=>void;
  native.mockReturnValueOnce(new Promise(done=>{resolve=done;}));
  render(<ServerBackupCard disabled={false}/>);
  expect(screen.getByRole("status").textContent).toContain("불러오는 중이에요");
  await act(async()=>{await Promise.resolve();});
  await act(async()=>{resolve({operation_id:"abcdefab-1234-1234-1234-123456789abc",phase:"failed",snapshot_at:null,bytes:null,failure_reason:"storage_full"});});
  expect(screen.getByRole("status").textContent).toContain("저장 공간이 부족해요");
});
