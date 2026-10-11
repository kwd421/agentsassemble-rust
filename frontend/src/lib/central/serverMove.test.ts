import {afterEach,expect,it,vi} from "vitest";
import {switchServerRestore,resumeServerRestore,type RestoreJournal} from "./serverMove";
const mocks=vi.hoisted(()=>({native:vi.fn(),bootstrap:vi.fn(),signed:vi.fn(),sign:vi.fn(),session:{token:"session",person:{person_id:"owner",identity_kind:"guest"}} as object|null}));
vi.mock("../desktopBridge",()=>({requestDesktopServerRestore:mocks.native}));
vi.mock("./identity",()=>({bootstrapCentral:mocks.bootstrap,loadCentralSession:()=>mocks.session,signedRequest:mocks.signed,signCentralNativeRequest:mocks.sign}));
vi.mock("./accountDeletion",()=>({parseDeletionProof:(value:unknown,id:string)=>{const proof=value as {request_id:string;proof:string};if(proof.request_id!==id)throw new Error("wrong proof purpose");return proof;}}));
const id="30000000-0000-4000-8000-000000000001",operation="30000000-0000-4000-8000-000000000002";
const job:RestoreJournal={operation_id:operation,phase:"prepared",expected_epoch:"",new_key_fingerprint:"",committed_epoch:"",manifest:{server_id:id,server_name:"Server",rooms:2,messages:3,assets:1,snapshot_at:new Date().toISOString()}};
afterEach(()=>{vi.resetAllMocks();mocks.session={token:"session",person:{person_id:"owner",identity_kind:"guest"}};});
it("requires live ownership before asking native to bind or switching",async()=>{
  mocks.bootstrap.mockResolvedValue({servers:[{server_id:id,relation:"member",registration_epoch:"epoch"}]});
  await expect(switchServerRestore(job,"recovery",()=>{},new AbortController().signal,"epoch")).rejects.toThrow(/소유권/);
  expect(mocks.native).not.toHaveBeenCalled();
});
it("signs the exact native-owned body and leaves activation to native receipt handling",async()=>{
  mocks.bootstrap.mockResolvedValue({servers:[{server_id:id,relation:"owner",registration_epoch:"epoch"}]});
  const raw='{"exact_native_body":"preserved"}';
  const seen:Record<string,unknown>[]=[];
  mocks.native.mockImplementation(async(request:Record<string,unknown>)=>{seen.push(request);
    if(request.action==="bind")return{path:`/v1/servers/${id}/restore-proof`,request_id:"r".repeat(43),body:{native_tuple:true}};
    if(request.action==="draft")return{path:`/v1/servers/${id}/restore`,body:raw};
    if(request.action==="switch")return{status:"committed"};
    if(request.action==="inspect")return{...job,phase:"activated",database_hash:null,key_hash:null,endpoint_generation:4000000000000};
    throw new Error("unexpected client orchestration");
  });
  mocks.signed.mockResolvedValue({request_id:"r".repeat(43),proof:"p".repeat(43)});
  mocks.sign.mockImplementation(async(_session,path,method,body)=>{expect([path,method,body]).toEqual([`/v1/servers/${id}/restore`,"POST",raw]);return{signature:"device"};});
  expect((await switchServerRestore(job,"recovery",()=>{},new AbortController().signal,"epoch"))?.phase).toBe("activated");
  expect(seen.find(request=>request.action==="switch")).toEqual({action:"switch",operation_id:operation,body:raw,headers:{signature:"device"}});
  expect(seen.some(request=>request.action==="activate")).toBe(false);
});
it("does not turn unknown or superseded central receipts into cancellation or success",async()=>{
  mocks.sign.mockResolvedValue({signature:"device"});
  for(const status of ["unknown","superseded"]){
    mocks.native.mockImplementation(async request=>request.action==="status_draft"?{path:`/v1/servers/${id}/restore-operations/${operation}`,body:"{}"}:{status});
    await expect(resumeServerRestore({...job,phase:"switch_unknown"})).rejects.toThrow();
    expect(mocks.native.mock.calls.at(-1)?.[0].action).toBe("status");
  }
});
