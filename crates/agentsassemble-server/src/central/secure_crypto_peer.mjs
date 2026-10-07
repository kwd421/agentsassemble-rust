import { createInterface } from 'node:readline';
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const read = async () => JSON.parse((await lines.next()).value);
const out = value => process.stdout.write(JSON.stringify(value) + '\n');
const b64 = b => Buffer.from(b).toString('base64url');
const bytes = s => Buffer.from(s, 'base64url');
const utf8 = s => new TextEncoder().encode(s);
const pinned = await read();
const pair = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits']);
const client = { protocol: 'secure_admission_v1', server_id: pinned.server_id, registration_epoch: 'epoch',
  origin: 'https://secure.example.test', generation: 1, purpose: 'owner',
  client_nonce: b64(crypto.getRandomValues(new Uint8Array(32))), client_public_key: b64(await crypto.subtle.exportKey('raw', pair.publicKey)) };
out(client);
const hello = await read();
const transcript = utf8(JSON.stringify(['AA-SECURE-ADMISSION-1', ...['protocol','server_id','registration_epoch','origin','generation','purpose','client_nonce','client_public_key','host_ephemeral_public_key','channel_id','issued_at'].map(k => hello[k])]));
const host = await crypto.subtle.importKey('raw', bytes(pinned.key), 'Ed25519', false, ['verify']);
if (!await crypto.subtle.verify('Ed25519', host, bytes(hello.signature), transcript)) throw Error('invalid host signature');
const peer = await crypto.subtle.importKey('raw', bytes(hello.host_ephemeral_public_key), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
const shared = await crypto.subtle.deriveBits({ name: 'ECDH', public: peer }, pair.privateKey, 256);
const material = await crypto.subtle.importKey('raw', shared, 'HKDF', false, ['deriveKey']);
const key = direction => crypto.subtle.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt: bytes(client.client_nonce),
  info: Buffer.concat([transcript, utf8(direction)]) }, material, { name: 'AES-GCM', length: 256 }, false, ['encrypt','decrypt']);
const aad = direction => utf8(JSON.stringify([client.protocol, hello.channel_id, direction, '0']));
const cipher = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: new Uint8Array(12), additionalData: aad('c2h') }, await key('c2h'), utf8('client possession'));
out(b64(Buffer.concat([Buffer.alloc(8), Buffer.from(cipher)])));
const record = bytes(await read());
const decrypted = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: new Uint8Array(12), additionalData: aad('h2c') }, await key('h2c'), record.subarray(8));
out(new TextDecoder().decode(decrypted));
process.exit(0);
