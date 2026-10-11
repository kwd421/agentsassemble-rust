import {expect,it,vi} from "vitest";
import {previewSecureMember,SecureMemberAdmission} from "./secureMemberEntry";
const mocks=vi.hoisted(()=>({signed:vi.fn(),connect:vi.fn(),credential:vi.fn(()=>"device")}));
vi.mock("./identity",()=>({loadCentralSession:()=>({token:"live"}),signedRequest:mocks.signed}));
vi.mock("../remote/remoteTransport",()=>({RemoteTransport:{connect:mocks.connect}}));
vi.mock("../deviceIdentity",()=>({getOrCreateBrowserCredential:mocks.credential,getOrCreateClientId:()=>"client"}));
it("pins restored invite consent and device admission to the resolved epoch while ordinary reconnect rejects drift",async()=>{
  const entry={server_id:"server",registration_epoch:"old",inviteToken:"retained-invite"};
  mocks.signed.mockResolvedValue({server_id:"server",registration_epoch:"new",label:"Server",endpoint_origin:"https://new.example",
    endpoint_generation:42,protocol:"secure_admission_v1",mode:"event_secure_v1",host_key_fingerprint:"fingerprint",host_public_key_jwk:{}});
  const preview=await previewSecureMember(entry);
  expect(mocks.signed.mock.calls[0][3]).toEqual({protocol:"secure_admission_v1",registration_epoch:"old",resolve_invite:true});
  const close=vi.fn();mocks.connect.mockResolvedValue({close});
  const admission=await SecureMemberAdmission.open(entry,preview.target,"live",new AbortController().signal);
  expect(mocks.connect.mock.calls[0][0].registration_epoch).toBe("new");
  expect(mocks.credential).toHaveBeenCalledWith({server_id:"server",registration_epoch:"new"});
  admission.close();expect(close).toHaveBeenCalledOnce();
  await expect(previewSecureMember({...entry,inviteToken:undefined})).rejects.toThrow();
  await expect(SecureMemberAdmission.open({...entry,inviteToken:undefined},preview.target,"live",new AbortController().signal)).rejects.toThrow();
});
