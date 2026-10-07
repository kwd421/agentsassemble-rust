import assert from 'node:assert/strict';
import { createInterface } from 'node:readline';
import { pathToFileURL } from 'node:url';
const { RemoteTransport } = await import(pathToFileURL(process.env.AA_REMOTE_TRANSPORT_MODULE));
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const config = JSON.parse((await lines.next()).value);
const NativeWebSocket = globalThis.WebSocket;
// Test-only physical tunnel adapter; production crypto, multiplexing and HTTP are unchanged.
globalThis.WebSocket = class extends NativeWebSocket {
  constructor(url) { assert.equal(String(url), `${config.target.origin.replace('https:', 'wss:')}/api/secure-channel`); super(`ws://${config.proxy}/api/secure-channel`); }
};
const remote = await RemoteTransport.connect(config.target, 'owner');
const headers = { 'content-type':'application/json', 'x-device-token':config.device };
async function post(path, body, extra={}) {
  const response = await remote.fetch(path, { method:'POST', headers:{...headers,...extra}, body:JSON.stringify(body) });
  const value = await response.json();
  assert.equal(response.status, 200, `${path}: ${JSON.stringify(value)}`); return value;
}
const denied = await remote.fetch('/api/runtime/version'); assert.equal(denied.status, 403); await denied.text();
const owner = await post('/api/central-owner/session', { grant_token:config.grant, generation:config.target.generation,
  device:{device_name:'Encrypted test',browser:'Node WebCrypto',os:'test'} });
const root = {session_token:owner.session_token,generation:config.target.generation};
const directory = await post('/api/central-owner/directory', root); assert.equal(directory.rooms.length, 0);
const personaHeaders = {authorization:`Bearer ${owner.session_token}`, 'x-central-generation':String(config.target.generation), 'x-device-token':config.device};
const imported = await post('/api/central-owner/personas/import', {filename:'Harbor Guide.png',data_base64:config.persona_png}, personaHeaders);
assert.equal(imported.persona.id, 'Harbor-Guide');
const personas = await remote.fetch('/api/central-owner/personas', {headers:personaHeaders});
assert.equal(personas.status,200); assert.equal((await personas.json()).items[0].id,'Harbor-Guide');
const thumbnail = await remote.fetch('/api/central-owner/personas/Harbor-Guide/thumbnail', {headers:personaHeaders});
assert.equal(thumbnail.status,200); assert.equal(thumbnail.headers.get('cache-control'),'private, no-store');
assert.deepEqual([...new Uint8Array(await thumbnail.arrayBuffer()).slice(0,8)], [137,80,78,71,13,10,26,10]);
const created = await post('/api/central-owner/rooms', {...root, request_id:crypto.randomUUID(),room_id:'secure-room',label:'Encrypted room'});
const listing = await post('/api/central-owner/directory',root);
assert.equal(listing.rooms.length,1);
const room = listing.rooms[0];
const joined = await post('/api/central-owner/room', {...root,room_id:'secure-room',room_uid:room.room_uid});
const ticket = await post('/api/session-tickets/socket',{device:{device_name:'Encrypted test',browser:'Node WebCrypto',os:'test'}},{authorization:`Bearer ${joined.session_token}`});
const socket = remote.openSocket(ticket.ticket);
const messages = [], waiters=[];
socket.onmessage = event => { const frame=JSON.parse(event.data); const waiter=waiters.shift(); if(waiter) waiter(frame); else messages.push(frame); };
const receive = () => messages.length ? Promise.resolve(messages.shift()) : new Promise(resolve=>waiters.push(resolve));
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
socket.send(JSON.stringify({op:'subscribe',streams:['room_events'],resume_from_seq:0}));
let snapshot;
for(let i=0;i<10;i++){const frame=await receive(); if(frame.op==='snapshot'){snapshot=frame;break;}}
assert.ok(snapshot,'room snapshot');
socket.send(JSON.stringify({op:'command',request_id:crypto.randomUUID(),action:'message.send',payload:{content:'encrypted hello'}}));
let ack=false,event=false;
for(let i=0;i<10;i++){const frame=await receive(); if(frame.op==='ack')ack=frame.accepted; if(frame.op==='event')event=frame.events.some(e=>e.content==='encrypted hello'); if(ack&&event)break;}
assert.ok(ack&&event,'message commit and delivery');
const stream = await remote.fetch('/api/central-owner/events',{method:'POST',headers,body:JSON.stringify(root)});
assert.equal(stream.status,200); const reader=stream.body.getReader(); assert.ok((await reader.read()).value.length); await reader.cancel();
assert.equal((await post('/api/central-owner/directory',root)).rooms.length,1,'outer channel retains owner after SSE cancel');
const wrong = await RemoteTransport.connect(config.target,'owner');
const stolen = await wrong.fetch('/api/central-owner/session',{method:'POST',headers,body:JSON.stringify({grant_token:config.grant,generation:config.target.generation,device:{device_name:'Other',browser:'Node',os:'test'}})});
assert.equal(stolen.status,401); await stolen.text(); wrong.close();

// The existing invitation, Joined transaction, reconnect selection and socket
// owner all run over the same production encrypted transport for members.
const invitation = await post('/api/central-owner/room-invite/create', {meeting_id:'secure-room',display_name:'Guest',invite_scope:'room',ttl_seconds:600,max_uses:2}, {authorization:`Bearer ${joined.session_token}`});
assert.equal(new URL(invitation.join_url).pathname, '/member-join');
assert.equal(new URL(invitation.join_url).searchParams.get('protocol'), 'secure_admission_v1');
const memberDevice = 'aad1.B'.replace('.', '_') + 'A'.repeat(42);
async function memberPost(channel,path,body) {
  const response=await channel.fetch(path,{method:'POST',headers:{'content-type':'application/json','x-device-token':memberDevice},body:JSON.stringify(body)});
  const value=await response.json(); assert.equal(response.status,200,`${path}: ${JSON.stringify(value)}`); return value;
}
const member=await RemoteTransport.connect(config.target,'member_admission');
const challenge=await memberPost(member,'/api/room-invite/member-challenge',{invite_token:invitation.join_code});
const preflight=await memberPost(member,'/api/room-invite/admission',{invite_token:invitation.join_code}); assert.equal(preflight.room_id,'secure-room');
const memberJoined=await memberPost(member,'/api/room-invite/member-join',{invite_token:invitation.join_code,challenge_id:challenge.challenge_id,grant_token:'aamg1.'+'A'.repeat(43),request_id:crypto.randomUUID(),client_id:'secure-member-client'});
assert.equal(memberJoined.status,'admitted');
for (const credential of [memberJoined.session_token, owner.session_token]) {
  for (const path of ['/api/central-owner/personas','/api/central-owner/personas/Harbor-Guide/thumbnail','/api/central-owner/personas/import']) {
    const rejected = await member.fetch(path, { method:path.endsWith('/import')?'POST':'GET', headers:{...personaHeaders,authorization:`Bearer ${credential}`} });
    assert.equal(rejected.status,401); await rejected.text();
  }
}
const wrongChannel=await member.fetch('/api/session-tickets/socket',{method:'POST',headers:{'x-device-token':config.device,authorization:`Bearer ${joined.session_token}`},body:'{}'});
assert.equal(wrongChannel.status,401); await wrongChannel.text();
const memberTicketResponse=await member.fetch('/api/session-tickets/socket',{method:'POST',headers:{'x-device-token':memberDevice,authorization:`Bearer ${memberJoined.session_token}`} });
assert.equal(memberTicketResponse.status,200); const memberTicket=await memberTicketResponse.json();
const memberSocket=member.openSocket(memberTicket.ticket);
await new Promise((resolve,reject)=>{memberSocket.onopen=resolve;memberSocket.onerror=reject;});
memberSocket.close(); member.close();
const reconnect=await RemoteTransport.connect(config.target,'member_connect');
const connectChallenge=await memberPost(reconnect,'/api/member-connect/challenge',{});
const available=await memberPost(reconnect,'/api/member-connect/rooms',{challenge_id:connectChallenge.challenge_id,grant_token:'aamc1.'+'A'.repeat(43)});
assert.equal(available.rooms[0].room_id,'secure-room');
const selected=await memberPost(reconnect,'/api/member-connect/select',{challenge_id:connectChallenge.challenge_id,room_id:'secure-room',client_id:'secure-member-client'});
assert.equal(selected.status,'admitted'); reconnect.close();
const late=await RemoteTransport.connect(config.target,'owner');
const lateResult=late.fetch('/api/central-owner/session',{method:'POST',headers,body:JSON.stringify({grant_token:config.late_grant,generation:config.target.generation,device:{device_name:'Late admission',browser:'Node',os:'test'}})}).then(()=>{throw new Error('late admission unexpectedly returned');},()=>{});
console.log(JSON.stringify({owner:owner.session_token,room:joined.session_token,channel:remote.hello.channel_id,late_channel:late.hello.channel_id,late_key:late.hello.client_public_key}));
await lines.next();
late.close(); socket.close(); remote.close();
await lateResult;
console.log('closed');
process.exit(0);
