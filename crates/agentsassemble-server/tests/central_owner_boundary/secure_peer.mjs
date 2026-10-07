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
console.log(JSON.stringify({owner:owner.session_token,room:joined.session_token,channel:remote.hello.channel_id}));
await lines.next();
socket.close(); remote.close();
console.log('closed');
process.exit(0);
