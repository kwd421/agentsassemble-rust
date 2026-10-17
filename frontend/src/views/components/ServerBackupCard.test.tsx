import {act,cleanup,fireEvent,render,screen} from "@testing-library/react";
import {afterEach,expect,it,vi} from "vitest";
import ServerBackupCard from "./ServerBackupCard";
const native=vi.hoisted(()=>vi.fn());
const local=vi.hoisted(()=>({server_id:"server",hosting_state:null as string|null,host_key_fingerprint:"current-key"}));
const probe=vi.hoisted(()=>({fetchError:null as unknown|null,servers:null as null|Array<{server_id:string;relation:string;alias:string;host_key_fingerprint:string}>}));
vi.mock("../../lib/desktopBridge",()=>({
  isDesktopWebview:()=>true,
  requestDesktopServerBackup:native,
}));
vi.mock("../../lib/central/identity",()=>({
  bootstrapCentral:async()=>({servers:probe.servers??[{server_id:"server",relation:"owner",alias:"Owner server",host_key_fingerprint:"current-key"}]}),
  fetchLocalServerInfo:async()=>{if(probe.fetchError)throw probe.fetchError;return local;},
  localHostingState:async()=>local.hosting_state,
  loadCentralSession:()=>({person:{person_id:"owner"}}),
}));
afterEach(()=>{cleanup();vi.useRealTimers();native.mockReset();local.hosting_state=null;local.host_key_fingerprint="current-key";probe.fetchError=null;probe.servers=null;});
// A lost status response must not strand a recovered running export after retry.
it("recovers the runtime job on remount and restarts status observation after transport failure",async()=>{
  vi.useFakeTimers();
  const progress={operation_id:"abcdefab-1234-1234-1234-123456789abc",phase:"encoding",snapshot_at:null,bytes:null};
  native.mockResolvedValueOnce(progress).mockRejectedValueOnce(new Error("connection lost"))
    .mockResolvedValueOnce(progress).mockResolvedValueOnce({...progress,phase:"complete",snapshot_at:"2026-10-11T00:00:00Z",bytes:1024});
  await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
  expect(screen.getByRole("status").textContent).toContain("백업 파일을 만들고 있어요");
  await act(async()=>{await vi.advanceTimersByTimeAsync(1000);});
  expect(screen.getByRole("alert").textContent).toContain("백업 상태를 불러오지 못했어요.");
  expect(screen.getByRole("alert").textContent).not.toContain("connection lost");
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

for(const scenario of ["retired","device","account_deleted","replaced-key"]) {
  it(`hides backup and old export failure on ${scenario} computer`,async()=>{
    if(scenario==="replaced-key")local.host_key_fingerprint="old-key";
    else local.hosting_state=scenario;
    native.mockResolvedValue({operation_id:"abcdefab-1234-1234-1234-123456789abc",phase:"failed",snapshot_at:null,bytes:null,failure_reason:"storage_full"});
    await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
    expect(screen.queryByRole("region",{name:"서버 백업"})).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    expect(native).not.toHaveBeenCalled();
  });
}

it("stays silent when the local runtime is not up yet at mount",async()=>{
  probe.fetchError=new TypeError("Load failed");
  await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
  expect(screen.queryByRole("region",{name:"서버 백업"})).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(document.body.textContent).not.toContain("Load failed");
});

it("renders nothing for a signed-in account that does not own this server",async()=>{
  probe.servers=[{server_id:"server",relation:"member",alias:"Other server",host_key_fingerprint:"current-key"}];
  native.mockResolvedValue(null);
  await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
  expect(screen.queryByRole("region",{name:"서버 백업"})).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("maps a pressed backup transport failure to one Korean line",async()=>{
  native.mockResolvedValueOnce(null).mockRejectedValueOnce(new TypeError("Load failed")).mockResolvedValueOnce(null);
  await act(async()=>{render(<ServerBackupCard disabled={false}/>);});
  const password=screen.getByLabelText("비밀번호");
  const confirm=screen.getByLabelText("비밀번호 확인");
  await act(async()=>{fireEvent.change(password,{target:{value:"secret-123"}});fireEvent.change(confirm,{target:{value:"secret-123"}});});
  await act(async()=>{fireEvent.click(screen.getByText("백업 파일 저장"));});
  const alert=await screen.findByRole("alert");
  expect(alert.textContent).toContain("백업을 시작하지 못했어요.");
  expect(alert.textContent).not.toContain("Load failed");
  expect(alert.textContent).not.toMatch(/\n/);
});
